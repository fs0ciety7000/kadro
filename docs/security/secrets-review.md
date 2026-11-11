# Revue du modèle de secrets — Kropa (S.2)

> Tâche S.2, `security-reviewer`, 11/11/2026. Relu : `security/secrets.md` (S.1), `decisions.md` (prioritaire),
> `uploads.md` § 4 et 7, `platform-macos.md` § 4, `platform-linux.md`, `cli.md`, `architecture.md` (IPC). Vérifié sur
> sources primaires [1]–[8] : spec age, doc et code des crates retenus (versions de S.1 confirmées sur crates.io),
> `securityd` d'Apple, `std` Rust, pages de manuel Linux. Aucune dépendance de stockage encore (`Cargo.lock` : `clap`).

## Verdict

**Non validable en l'état : 2 bloquants**, 10 importants, 13 mineurs. Les fondations sont saines : trousseau d'abord,
`age` standard sans assemblage maison, origines dans la valeur protégée, aucune bascule silencieuse, ni argument ni
variable d'environnement, refus avant connexion. Les deux bloquants contournent par simple config la promesse
centrale (« un secret ne va qu'à son origine, jamais dans le lien »). S.3 : corriger B1 et B2, puis chaque important
avant le lot indiqué. Le constat macOS de S.1 est confirmé par le code d'Apple (§ macOS).

## Bloquants

**B1. Sorties tirées de la réponse non contrôlées** (§ 4, § 6 ; menace « Secret recopié dans le lien »). Seul
`{secret:}` est interdit dans les gabarits de réponse. Or la query est un emplacement permis et `{responseurl}` rend
l'URL finale, `?key=<jeton>` compris sans redirection ; `{response}`, `{json:}`, `{regex:}`, `{header:}` capturent un
jeton renvoyé en écho. L'expurgation ne vise que journaux et `--json` : le jeton part au presse-papiers, dans SQLite,
sur stdout. Le test prévu ne couvre aucun de ces cas. *Correction* : `{responseurl}` toujours sans query ni fragment ;
contrôle final de **toute** valeur sortante (`url`, `thumbnail_url`, `deletion_url`, `error`, texte du lien,
historique, presse-papiers, notification, stdout) contre les secrets résolus, en formes brute, percent-encodée,
base64 et échappée JSON → code 11, rien n'est copié ni enregistré. *Tests* `wiremock` : écho du jeton +
`url = "{json:$.echo}"` → code 11, base et presse-papiers factice vides ; secret en query + `{responseurl}` → sans query.

**B2. Import TOML et références entre profils non couverts** (§ 4, § 5 ; `cli.md` § 2.3). `dest import x.toml`
existe, mais « références toujours neuves » ne vise que `.sxcu`, et `ref = "<profil>/<nom>"` laisse un profil pointer
le secret d'un autre. La liaison par origine ne fixe pas l'**emplacement** : une config tierce visant la même origine
déplace `{secret:token}` d'`Authorization` vers la query (journaux d'accès du serveur et des proxys, puis B1) ou vers
un champ que le serveur stocke et affiche (`x-zipline-original-name`, titre, dossier). *Correction* : (1) références
limitées au profil courant (`ref = "token"`), aucune référence inter-profils ; (2) import TOML aux règles de `.sxcu`
(entrées neuves, valeurs en clair extraites, aucun écrasement de profil sans confirmation interactive) ;
(3) emplacement (`header:Authorization`, `query:key`, `form:token`, `body`) stocké avec `origins` dans la valeur
protégée (§ 2.3), vérifié à la résolution, modifiable seulement comme une origine. *Tests* : TOML importé visant
`autre/token` → refus au chargement ; secret déplacé vers la query → zéro requête reçue.

## Importants

- **I1. Phrase de passe sans robustesse minimale** (§ 3, avant 2.8). scrypt 2^18 ne protège pas « kropa2026 » d'une
  sauvegarde volée. *Correction* : refus si score `zxcvbn` (3.1.1) < 3 ou < 12 caractères ; phrase de 6 mots diceware
  (≈ 77 bits) proposée. *Test* : phrase faible refusée à la création, coffre existant toujours lisible.
- **I2. Verrou perdu au `rename`** (§ 3, avant 2.8). `flock` sur `secrets.age` tient l'ancien inode : un écrivain en
  attente relit l'ancien fichier, et le résident peut réécrire depuis son cache → entrées perdues. *Correction* :
  verrou sur un `secrets.lock` stable (`0600`), relecture **après** prise du verrou, jamais d'écriture depuis le cache ;
  résident lancé = la CLI écrit par l'IPC. *Test* : deux écrivains × 100 écritures → aucune entrée perdue.
- **I3. IPC : serveur non authentifié, repli non défini** (§ 1, avant 2.1/4.7). Seul le serveur vérifie le pair. Un
  repli vers `/tmp` quand `XDG_RUNTIME_DIR` manque (su, cron, SSH sans pam_systemd) laisse un autre utilisateur
  squatter la socket et recevoir `secrets.unlock`/`secrets.set`. *Correction* : le client vérifie
  `peer_cred().uid == getuid()` (tokio, Linux et macOS) et le dossier (propriétaire, `0700`, pas de lien) avant tout
  envoi ; pas de repli (code 14, mode par processus) ; un seul chemin macOS `0700` (`architecture.md` dit
  `~/Library/Caches/Kropa/`, à aligner) ; le dossier est le vrai contrôle, les droits d'un fichier socket ne sont pas
  portables [8] ; ligne JSON-RPC ≤ 64 Kio ; paramètres `secrets.*` jamais journalisés. *Test* : le test « namespace »
  est inopérant, les identifiants passés par socket sont traduits vers l'espace du récepteur [8] (même UID vu) ; le
  remplacer par des identifiants injectés en test unitaire + un job CI `sudo -u nobody`, dans les deux sens.
- **I4. Exception HTTP de bouclage exploitable** (§ 4, avant 1.7). Un autre utilisateur local peut écouter un port
  libre de 127.0.0.1 quand le service légitime est arrêté et recevoir le jeton en clair ; `localhost` passe par le
  résolveur. *Correction* : exception réservée aux builds de test (`cfg(test)` ou fonction non livrée) ; si on la garde
  pour l'utilisateur : option par profil, IP littérales seulement, avertissement à chaque envoi.
- **I5. Trousseau Linux : blocage possible en non interactif** (§ 2.1, avant 1.8). Le magasin 1.0.1 appelle
  `unlock_all` (prompt) dès qu'un élément est verrouillé et attend la réponse [3] : avec une session graphique, un
  script attend un humain, contrairement au « aucun blocage » du § 3. `search_items` parcourt toutes les collections :
  un doublon donne `Ambiguous`. *Correction* : sans TTY ou en `--no-input`, lire `Locked` de la collection et échouer
  `keyring_locked` sans prompt ; API bloquante en `spawn_blocking` avec délai ; `Ambiguous` expliqué par `doctor`.
  *Test* : collection verrouillée + `--no-input` → code 7 en moins de 2 s.
- **I6. macOS : deux binaires, deux identités** (§ 2.2, avant 1.6/1.8). `kropa` et `kropa-app` ont des cdhash (et des
  DR si leurs identifiants diffèrent) distincts : un élément créé par l'un déclenche une invite pour l'autre, et chaque
  mise à jour une invite par binaire. *Correction* : un seul élément (Q5), protocole 1.6 sur les deux binaires, doc
  « jusqu'à 2 invites par mise à jour » ; jamais `apple-tool:` dans la liste de partitions : c'est celle de l'outil
  `security(1)` [5], qui lirait alors l'élément sans invite.
- **I7. Expurgation incomplète** (§ 6, avant 1.7/1.9). (a) Formes dérivées : `{base64:}` (Basic), percent-encodage,
  échappement JSON → la passe finale cherche aussi ces formes ; (b) `-vv` n'ouvre que les cibles `kropa*`, `hyper`,
  `h2`, `reqwest`, `rustls`, `russh`, `zbus` restent à `info` ; (c) `HeaderValue::set_sensitive(true)` sur tout
  en-tête porteur (masqué en `Debug`, non indexé par HPACK). *Test* 1.9 : Basic renvoyé en base64 → `***`.
- **I8. `zeroize` promis au-delà du possible** (§ 3, avant 2.8). « Jamais `String` nu » est intenable : `serde_json`
  (allocations, croissance de `Vec`), `keyring` (`Vec<u8>` rendu), `rpassword` (`String`), `HeaderValue` (`Bytes`),
  tampons rustls laissent des copies. *Correction* : écrire « au mieux, tampons de Kropa seulement » ; tampons
  pré-alloués sans croissance, désérialisation empruntée (`&str`) puis copie en `SecretString`, tout retour enveloppé
  aussitôt ; retirer `mlock` (sans effet sur ces copies) ; recommander un swap chiffré dans la doc.
- **I9. Session du coffre mal bornée** (§ 3, avant 4.7). Si chaque envoi relance les 15 min, un envoi périodique garde
  le coffre ouvert sans fin. Le signal logind `Lock` est une *demande* (`loginctl lock-session`), pas l'avis d'un écran
  verrouillé ; la propriété `LockedHint` de la session l'est (à vérifier par bureau). *Correction* : inactivité = aucune
  action humaine ; plafond absolu (8 h) ; verrouillage sur `LockedHint`, sur `PrepareForSleep` avec inhibiteur
  `sleep:delay` (avant hibernation), en fin de session. *Test* : bus D-Bus privé qui émet ces signaux ; remplacer
  « inspection du tampon » (irréaliste en Rust sûr) par état `None` + compteur de `Drop` du type secret.
- **I10. Secret dans le chemin : § 4 contredit § 5** (avant 1.7). § 4 interdit `{secret:}` dans le chemin, § 5 extrait
  les « segments de `RequestURL` » (webhooks `/api/<id>/<jeton>`). *Correction* : refuser ces imports par un message
  explicite (recommandé), ou autoriser un segment avec les contrôles de B1 et l'URL expurgée partout.

## Mineurs

- M1. Sans identifiants, `object_store` passe à `InstanceCredentialProvider` (IMDS 169.254.169.254) [7] : refuser
  avant `build()` si une clé manque ; `with_http_connector` permet d'y brancher le client unique.
- M2. `reqwest` lit `HTTP(S)_PROXY`/`ALL_PROXY` par défaut, contre « aucune variable » : `.no_proxy()` en v1, ou doc.
- M3. CA privée (Zipline auto-hébergé) : prévoir `rustls-platform-verifier` (0.7.1) ou `ca_file` par profil, jamais
  « ignorer TLS ».
- M4. Origine calculée par `url::Url::origin()` (formes IPv4, ports par défaut, point final) ; `user:pass@` refusé.
- M5. Réponse hostile : crate `regex` seul (temps linéaire, `size_limit`), XML sans DTD ni entité externe
  (`roxmltree`), `thumbnail_url` jamais téléchargée d'office, caractères bidi et de largeur nulle refusés dans l'URL.
- M6. Coffre ouvert en `O_NOFOLLOW`, droits et propriétaire lus par `fstat` sur le descripteur, dossier parent vérifié,
  `*.tmp` orphelins retirés ; base SQLite (URL de suppression, `partialToken`) en `0600`. `sync_all` fait déjà
  `F_FULLFSYNC` sur Apple [6].
- M7. `--password-fd`/`--secret-fd` : 4 Kio au plus, `CLOEXEC` (pas d'héritage par `kropa-app` lancé), refus de 1, 2 et
  d'un fd à double usage, seul un `\n` (ou `\r\n`) final retiré, avertissement si fichier ordinaire non `0600`.
- M8. `--var` pour un `{inputbox:}` placé dans un emplacement d'authentification met un secret en argument : le
  convertir en secret à l'import.
- M9. « Délai croissant après 3 essais » n'est pas une mitigation (l'attaque hors ligne coûte moins) : le garder pour
  l'ergonomie ; sérialiser les déverrouillages (256 Mio chacun).
- M10. Cohérence : « pas de `Serialize` » contredit l'écriture du coffre (type privé de `kropa-store`) ; le magasin
  Apple passe par `SecKeychain::set_generic_password` (recherche puis modification) [4], pas `SecItemUpdate` ;
  `cli.md` n'a ni `secrets allow`, ni `use`, ni `migrate` ; `allow` interactif seulement ; `set` montre l'origine liée.
- M11. `unsafe_code = "deny"` : `prctl`, `setrlimit`, `flock` par `rustix` (1.1.6), identifiants du pair par tokio.
- M12. Dialogue : proposer de vider le presse-papiers s'il contient encore le jeton collé ; X11 : frappe lisible par
  tout client, à documenter.
- M13. § 7 : la confirmation de changement d'origine n'arrête pas un processus du même utilisateur (il appelle l'IPC) ;
  le dire, pour ne pas la présenter comme une frontière.

## macOS : Keychain et certificat auto-signé (avis sourcé)

`secrets.md` § 2.2 est juste. `platform-macos.md` § 4.4 (« avec le certificat auto-signé stable, l'ACL reste
valide ») est vrai pour l'ACL, mais en déduit à tort l'absence d'invite. Source primaire : `securityd`,
`partitionIdForProcess` [5]. `unsigned:` si non signé ; `apple:`/`apple-tool:` si ancre Apple ; `teamid:`
**seulement** pour Mac App Store, TestFlight, Developer ID ou Apple Development ; tout autre code signé, **ad hoc
comme auto-signé**, reçoit `cdhash:<hash>`, qui change à chaque build. La DR stable garde l'ACL valide, la partition
ne correspond plus → invite après chaque mise à jour, par élément et par binaire ; « Toujours autoriser » ajoute le
nouveau cdhash. TCC ne lit que la DR : le certificat auto-signé y garde tout son intérêt. Limites : branche `main` du
code ouvert, pas un binaire 15/26/27 ; texte de l'invite et mot de passe demandé ou non à relever en 1.6 (builds N
puis N+1 des deux binaires, même certificat ; liste de partitions de l'élément relevée avant et après).
À corriger en S.3 : `platform-macos.md` § 4.4 et la phrase de repli de `decisions.md` (invite Keychain maintenue).

## Modèle de menaces : réalisme des tests

Réalistes tels quels : bascule silencieuse (`dbus-run-session` sans démon), redirection, `http://` refusé, réponse de
2 Mio, argument, environnement, SFTP, fichier piégé (age refuse avant calcul [2] ; ajouter `log2 N` = 21 refusé et
stanza scrypt mêlée à une autre refusée [1]).

| Ligne | Problème | Test à ajouter ou remplacer |
|---|---|---|
| Secret lu sur disque | grep sur trois fichiers | config, données, cache, journaux et `*.tmp` |
| Coffre lisible | `0644` seul | lien symbolique, dossier `0755`, propriétaire étranger (`sudo` en CI) |
| Coffre tronqué | pas de concurrence | deux écrivains (I2) |
| Attaque hors ligne | prouve le facteur, pas la phrase | refus de phrase faible (I1) |
| TOML malveillant | origine seule | référence inter-profils, emplacement déplacé, import TOML (B2) |
| Interception | pas de TLS réel | serveur TLS à certificat `rcgen` non approuvé → échec |
| Secret dans le lien | `{secret:}` en réponse seulement | écho + `{json:}`, query + `{responseurl}` (B1) |
| Serveur qui renvoie le jeton | journaux et `--json` | presse-papiers, SQLite, stdout ; base64, percent (I7) |
| `Debug`/panique | pas de panique | panique provoquée pendant un envoi → stderr sans valeur |
| Socket IPC | `namespace` inopérant [8] | identifiants injectés + `sudo -u nobody`, deux sens (I3) |
| Coffre laissé ouvert | inspection mémoire irréaliste | compteur de `Drop`, signaux D-Bus simulés (I9) |
| Invites Keychain | protocole vague | deux binaires, deux builds, `partition_id` relevé |
| `.sxcu` partagé | une fixture | jeton dans le chemin (I10), dans `Data`, argument au nom inhabituel |

Menaces absentes : phrase faible, écrivains concurrents, serveur IPC usurpé, port de bouclage squatté, trousseau qui
bloque un script, emplacement déplacé, formes encodées dans les journaux, réponse XML piégée.

## Réponses aux questions ouvertes

1. **Keychain et auto-signé** : oui, invite à chaque mise à jour ([5]). Ne **pas** mettre le coffre par défaut : il
   redemande sa phrase à chaque session, le Keychain une fois par mise à jour. Trousseau, un seul élément (Q5), notes
   de version, coffre proposé par `doctor`.
2. **Secret Service** : fonction `rt-tokio-crypto-rust` (aes, cbc, hkdf, sha2 ; pas d'OpenSSL). Le magasin 1.0.1
   impose `EncryptionType::Dh` [3] : pas de repli `plain`, un service sans DH échoue (`keyring_unavailable`, à
   tester). Ne rien ajouter : gain faible (bus du même utilisateur, groupe DH de 1024 bits), coût nul.
3. **Facteur scrypt** : 18 fixe (256 Mio), oui ; la calibration varie (19–20 sur machine rapide : 512 Mio–1 Gio et
   plusieurs secondes sur une lente). Plafond 20 (1 Gio) suffisant ; au-delà, refus avec la commande de réécriture.
   La marge réelle vient de la phrase (I1).
4. **Phrase en mémoire** : ne pas la garder. Le résident garde les valeurs déchiffrées ; toute écriture (`set`,
   `remove`, `allow`, `migrate`), rare, redemande la phrase. Si c'est trop gênant : identité X25519 chiffrée par phrase
   (format age standard, ouvrable par l'outil `age` avec cette identité), le résident garde l'identité, pas la phrase.
5. **Granularité** : un seul élément pour tout Kropa (service `kropa`, compte `store`) portant le même document JSON
   que le coffre (`entries`), sur les deux OS : une invite par mise à jour et par binaire, un seul modèle, un seul
   verrou (I2). Coût : moins lisible dans Seahorse.
6. **Origines S3** : non. Lier le secret au triplet (endpoint canonique, région, bucket) ; virtual-host et path-style
   en sont deux formes acceptées. SigV4 n'envoie jamais la clé secrète : le risque est l'usage des droits sur un autre
   bucket (écrasement d'objets), donc changer bucket, région ou endpoint = même confirmation qu'une origine.
7. **Entropie** : pas comme critère principal. Toute valeur non vide d'en-tête, d'argument, de paramètre ou de corps
   est secrète sauf liste blanche (`Accept`, `Content-Type`, `User-Agent`, gabarits purs comme `{filename}`), montrée
   masquée et reclassable ; avec `--yes`, tout ce qui sort de la liste blanche va au magasin. Entropie (≥ 3,5 bits par
   caractère sur ≥ 16) seulement pour signaler un segment de chemin (I10). Plus de faux négatif sur jeton court.
8. **`PR_SET_DUMPABLE(0)`** : le garder dans `kropa-app` en release et dans `kropa` tant qu'il tient un secret. Gain
   réel : un processus non dumpable n'est pas attachable par `ptrace` [8] (ni lisible par `/proc/<pid>/mem`) depuis un
   processus non root du même utilisateur. Coût faible : ni core dump ni gdb sans root, et Kropa n'a aucun rapport de
   plantage automatique. Désactivé sous `debug_assertions`, jamais par variable. macOS : le runtime renforcé (sans
   `get-task-allow`) bloque déjà l'attache.

## Non vérifié

- Invites Keychain réelles sur macOS 15, 26, 27 (1.6) ; `LockedHint` sur GNOME, KDE, wlroots ; échec effectif d'un
  service sans DH ; défaut de proxy de `reqwest` 0.13.
- Aucun audit RustSec : les crates de stockage ne sont pas ajoutées (`cargo deny` au moment de l'ajout).
- Git : `.env.example` suivi (prévu, non lu) ; aucun autre fichier de secrets suivi.

## Sources (consultées le 11/11/2026)

[1] https://github.com/C2SP/C2SP/blob/main/age.md — scrypt : stanza seule, « SHOULD apply an upper limit », exemple 18.
[2] https://docs.rs/age/0.12.1/age/scrypt/ — `Recipient` ≈ 1 s ; `Identity::set_max_work_factor`, > 22 : heures, Gio.
[3] `zbus-secret-service-keyring-store` 1.0.1, source du crate, `src/service.rs` (`connect(EncryptionType::Dh)`,
    `search_items`, `unlock_all`) ; fonctions `rt-{tokio,async-io}-crypto-{rust,openssl}` (docs.rs).
[4] `apple-native-keyring-store` 1.0.2 `src/keychain.rs` ; `security-framework` 3.7.0 `src/os/macos/passwords.rs`.
[5] https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp — `partitionIdForProcess`.
[6] https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/sys/fs/unix.rs — `fsync` = `F_FULLFSYNC` (Apple).
[7] `object_store` 0.14.2, source du crate, `src/aws/builder.rs` (`InstanceCredentialProvider`, `with_http_connector`).
[8] man7.org : `unix(7)` (droits de socket non portables), `user_namespaces(7)` (identifiants traduits),
    `PR_SET_DUMPABLE(2const)` et `ptrace(2)` (processus non dumpable non attachable).
