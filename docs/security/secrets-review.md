# Revue du modèle de secrets — Kropa (S.2)

> S.2, `security-reviewer`, 11/11/2026. Relu : `security/secrets.md`, `decisions.md`, `uploads.md` § 4 et 7,
> `platform-macos.md` § 4, `platform-linux.md`, `cli.md`, `architecture.md` (IPC). Vérifié sur sources primaires
> [1]–[8] (versions de S.1 confirmées sur crates.io). Aucune dépendance de stockage encore (`Cargo.lock`).

## Verdict

**Non validable en l'état : 2 bloquants**, 10 importants, 12 mineurs. Fondations saines : trousseau d'abord, `age`
standard, origines dans la valeur protégée, aucune bascule silencieuse, ni argument ni variable, refus avant
connexion. Les deux bloquants contournent par simple config la promesse « un secret ne va qu'à son origine, jamais
dans le lien ». S.3 : corriger B1 et B2, puis chaque important avant son lot. Constat macOS de S.1 confirmé (§ macOS).

## Bloquants

**B1. Sorties tirées de la réponse non contrôlées** (§ 4, § 6). Seul `{secret:}` est interdit en réponse. Or la query
est un emplacement permis et `{responseurl}` rend l'URL finale, `?key=<jeton>` compris sans redirection ; `{response}`,
`{json:}`, `{regex:}`, `{header:}` capturent un jeton renvoyé en écho. L'expurgation ne vise que journaux et `--json` :
le jeton part au presse-papiers, dans SQLite, sur stdout ; le test prévu ne couvre aucun de ces cas. *Correction* :
`{responseurl}` sans query ni fragment ; contrôle final de **toute** valeur sortante (`url`, `thumbnail_url`,
`deletion_url`, `error`, lien, historique, presse-papiers, notification, stdout) contre les secrets résolus, en formes
brute, percent-encodée, base64 et JSON → code 11, rien copié ni enregistré. *Tests* `wiremock` : écho +
`url = "{json:$.echo}"` → code 11, base et presse-papiers factice vides ; query secrète + `{responseurl}` → sans query.

**B2. Import TOML et références entre profils non couverts** (§ 4, § 5 ; `cli.md` § 2.3). `dest import x.toml` existe,
mais « références toujours neuves » ne vise que `.sxcu`, et `ref = "<profil>/<nom>"` laisse un profil pointer le
secret d'un autre. L'origine ne fixe pas l'**emplacement** : une config tierce sur la même origine déplace
`{secret:token}` vers la query (journaux du serveur et des proxys, puis B1) ou un champ stocké et affiché
(`x-zipline-original-name`, titre). *Correction* : (1) références limitées au profil courant (`ref = "token"`) ;
(2) import TOML aux règles de `.sxcu` (entrées neuves, valeurs en clair extraites, aucun écrasement de profil sans
confirmation interactive) ; (3) emplacement (`header:Authorization`, `query:key`, `form:token`, `body`) stocké avec
`origins` (§ 2.3), vérifié à la résolution, modifiable comme une origine. *Tests* : import visant `autre/token` →
refus ; secret déplacé → zéro requête.

## Importants

- **I1. Phrase sans robustesse minimale** (§ 3, avant 2.8). scrypt 2^18 ne sauve pas « kropa2026 » d'une sauvegarde
  volée. Refus si score `zxcvbn` (3.1.1) < 3 ou < 12 caractères ; phrase de 6 mots diceware (≈ 77 bits) proposée.
- **I2. Verrou perdu au `rename`** (§ 3, avant 2.8). `flock` sur `secrets.age` tient l'ancien inode : l'écrivain en
  attente relit l'ancien fichier, le résident peut réécrire depuis son cache → entrées perdues. Verrou sur un
  `secrets.lock` stable, relecture **après** le verrou, jamais d'écriture depuis le cache. *Test* : 2 écrivains × 100.
- **I3. IPC : serveur non authentifié, repli non défini** (§ 1, avant 2.1/4.7). Un repli vers `/tmp` sans
  `XDG_RUNTIME_DIR` (su, cron, SSH) laisse un autre utilisateur squatter la socket et recevoir `secrets.unlock`. Le
  client vérifie `peer_cred().uid == getuid()` (tokio) et le dossier (propriétaire, `0700`, pas de lien) ; pas de repli
  (code 14) ; un seul chemin macOS `0700` (`architecture.md` : `~/Library/Caches/`, à aligner) ; les droits du fichier
  socket ne sont pas portables [8] ; ligne ≤ 64 Kio ; `secrets.*` jamais journalisé. *Test* : « namespace » est
  inopérant (identifiants traduits vers l'espace du récepteur [8]) → identifiants injectés + CI `sudo -u nobody`.
- **I4. Bouclage HTTP exploitable** (§ 4, avant 1.7). Un autre utilisateur écoute un port libre de 127.0.0.1 quand le
  service est arrêté et reçoit le jeton en clair ; `localhost` passe par le résolveur. Exception réservée aux builds de
  test ; sinon option par profil, IP littérales seules, avertissement à chaque envoi.
- **I5. Trousseau Linux bloquant en non interactif** (§ 2.1, avant 1.8). Le magasin 1.0.1 appelle `unlock_all`
  (prompt) sur élément verrouillé et attend [3] : un script attend un humain, contre le « aucun blocage » du § 3.
  `search_items` parcourt toutes les collections : un doublon donne `Ambiguous`. Sans TTY, lire `Locked` et échouer
  `keyring_locked` sans prompt ; appels en `spawn_blocking` avec délai ; `Ambiguous` expliqué par `doctor`.
- **I6. macOS : deux binaires, deux identités** (§ 2.2, avant 1.6). `kropa` et `kropa-app` ont des cdhash (et des DR
  si identifiants distincts) différents : invite croisée, et une invite par binaire à chaque mise à jour. Un seul
  élément (Q5), protocole 1.6 sur les deux binaires ; jamais `apple-tool:` en partition (celle de `security(1)` [5]).
- **I7. Expurgation incomplète** (§ 6, avant 1.9). Formes dérivées (`{base64:}` de Basic, percent, JSON) cherchées
  aussi ; `-vv` n'ouvre que `kropa*` (`hyper`, `h2`, `reqwest`, `rustls`, `russh`, `zbus` à `info`) ;
  `HeaderValue::set_sensitive(true)` sur tout en-tête porteur (masqué en `Debug`, hors HPACK). *Test* : Basic en écho.
- **I8. `zeroize` surpromis** (§ 3, avant 2.8). `serde_json` (croissance de `Vec`), `keyring` (`Vec<u8>`), `rpassword`
  (`String`), `HeaderValue` (`Bytes`), rustls laissent des copies. Écrire « au mieux » ; tampons pré-alloués,
  désérialisation empruntée puis `SecretString` ; retirer `mlock` ; conseiller un swap chiffré.
- **I9. Session du coffre mal bornée** (§ 3, avant 4.7). Si chaque envoi relance les 15 min, un envoi périodique garde le
  coffre ouvert. logind `Lock` est une *demande* (`loginctl lock-session`), l'état est `LockedHint` (à vérifier par
  bureau). Inactivité = aucune action humaine ; plafond 8 h ; verrou sur `LockedHint`, `PrepareForSleep` (inhibiteur
  `sleep:delay`), fin de session. *Test* : bus D-Bus privé ; « inspection du tampon » → compteur de `Drop`.
- **I10. Secret dans le chemin : § 4 contredit § 5** (avant 1.7). § 4 l'interdit, § 5 extrait des « segments de
  `RequestURL` » (webhooks). Refuser ces imports avec un message explicite, ou autoriser un segment sous B1.

## Mineurs

- M1. Sans clé, `object_store` passe à l'IMDS (`InstanceCredentialProvider`) [7] : refuser avant `build()` ;
  `with_http_connector` y branche le client unique. M2. `reqwest` lit `HTTP(S)_PROXY` : `.no_proxy()` ou doc.
- M3. CA privée (Zipline auto-hébergé) : `rustls-platform-verifier` (0.7.1) ou `ca_file` par profil, jamais « ignorer ».
- M4. Origine par `url::Url::origin()` (formes IPv4, ports par défaut, point final) ; `user:pass@` refusé.
- M5. Réponse hostile : `regex` seul (linéaire, `size_limit`), XML sans DTD (`roxmltree`), `thumbnail_url` jamais
  téléchargée d'office, bidi et largeur nulle refusés dans l'URL.
- M6. Coffre en `O_NOFOLLOW`, contrôle par `fstat` du descripteur et du dossier, `*.tmp` orphelins retirés ; SQLite
  (URL de suppression) en `0600`. `sync_all` fait déjà `F_FULLFSYNC` sur Apple [6].
- M7. `--password-fd`/`--secret-fd` : 4 Kio max, `CLOEXEC`, refus de 1, 2 et d'un fd à double usage, un seul `\n` final
  retiré. M8. `--var` d'un `{inputbox:}` d'authentification = secret en argument : le convertir à l'import.
- M9. « Délai croissant » n'est pas une mitigation (attaque hors ligne) ; sérialiser les déverrouillages (256 Mio).
- M10. « Pas de `Serialize` » contredit l'écriture du coffre (type privé) ; le magasin Apple fait recherche puis
  modification [4], pas `SecItemUpdate` ; `cli.md` n'a ni `secrets allow`, ni `use`, ni `migrate` ; `allow` interactif.
- M11. `unsafe_code = "deny"` : `prctl`, `setrlimit`, `flock` par `rustix` (1.1.6), pair par tokio.
- M12. § 7 : la confirmation d'origine n'arrête pas un processus du même utilisateur (il appelle l'IPC) ; le dire.
  Dialogue : proposer de vider le presse-papiers s'il contient le jeton collé ; X11 : frappe lisible, à documenter.

## macOS : Keychain et certificat auto-signé (avis sourcé)

`secrets.md` § 2.2 a raison. `platform-macos.md` § 4.4 (« l'ACL reste valide ») est vrai pour l'ACL mais en déduit à
tort l'absence d'invite. Source primaire, `securityd` `partitionIdForProcess` [5] : `unsigned:` si non signé,
`apple:`/`apple-tool:` si ancre Apple, `teamid:` **seulement** pour Mac App Store, TestFlight, Developer ID ou Apple
Development ; tout autre code signé, **ad hoc comme auto-signé**, reçoit `cdhash:<hash>`, qui change à chaque build.
La DR stable valide l'ACL, la partition ne correspond plus → invite après chaque mise à jour, par élément et par
binaire ; « Toujours autoriser » ajoute le nouveau cdhash. TCC ne lit que la DR : le certificat y garde son intérêt.
Limites : code ouvert (branche `main`), pas un binaire 15/26/27 ; forme de l'invite (mot de passe ?) à relever en 1.6
(builds N puis N+1 des deux binaires, partitions relevées avant/après). S.3 : corriger `platform-macos.md` § 4.4.

## Modèle de menaces : réalisme des tests

Réalistes : bascule silencieuse (`dbus-run-session`), redirection, `http://`, 2 Mio, argument, environnement, SFTP,
fichier piégé (age refuse avant calcul [2] ; ajouter `log2 N` = 21 et scrypt mêlé à un autre stanza [1]).

| Ligne | Problème | Test à ajouter ou remplacer |
|---|---|---|
| Secret lu sur disque | trois fichiers | config, données, cache, journaux, `*.tmp` |
| Coffre lisible | `0644` seul | lien symbolique, dossier `0755`, autre propriétaire (`sudo` en CI) |
| Coffre tronqué ; hors ligne | ni concurrence, ni phrase | deux écrivains (I2) ; phrase faible refusée (I1) |
| TOML malveillant | origine seule | inter-profils, emplacement déplacé, import TOML (B2) |
| Interception | pas de TLS réel | serveur TLS, certificat `rcgen` non approuvé → échec |
| Lien ; jeton renvoyé | journaux, `--json` | écho + `{json:}`, `{responseurl}`, base64 ; SQLite, stdout (B1, I7) |
| `Debug`/panique | pas de panique | panique provoquée pendant un envoi → stderr sans valeur |
| Socket IPC | `namespace` inopérant [8] | identifiants injectés + `sudo -u nobody`, deux sens (I3) |
| Coffre laissé ouvert | inspection mémoire irréaliste | compteur de `Drop`, signaux D-Bus simulés (I9) |
| Invites Keychain ; `.sxcu` | protocole vague ; une fixture | deux binaires × deux builds ; jeton dans chemin, `Data` |

Menaces absentes : phrase faible, écrivains concurrents, serveur IPC usurpé, port de bouclage squatté, trousseau qui
bloque un script, emplacement déplacé, formes encodées dans les journaux, réponse XML piégée.

## Réponses aux questions ouvertes

1. **Keychain et auto-signé** : oui, invite à chaque mise à jour [5]. Coffre **pas** par défaut : il redemande sa phrase
   à chaque session, le Keychain une fois par mise à jour. Trousseau, un seul élément (Q5), notes de version.
2. **Secret Service** : `rt-tokio-crypto-rust` (aes, cbc, hkdf, sha2 ; sans OpenSSL). Le magasin 1.0.1 impose
   `EncryptionType::Dh` [3] : pas de repli `plain`, service sans DH = échec (à tester). Gain faible (bus du même
   utilisateur, DH 1024 bits), coût nul : ne rien ajouter.
3. **Facteur scrypt** : 18 fixe (256 Mio), oui ; la calibration donnerait 19–20 sur machine rapide (jusqu'à 1 Gio et
   des secondes sur une lente). Plafond 20 suffisant ; au-delà, refus avec la commande de réécriture. La marge vient
   surtout de la phrase (I1).
4. **Phrase en mémoire** : ne pas la garder. Le résident garde les valeurs ; toute écriture (`set`, `remove`, `allow`,
   `migrate`), rare, redemande la phrase. Repli si gênant : identité X25519 chiffrée par phrase (format age standard).
5. **Granularité** : un seul élément (`kropa` / `store`) portant le même document JSON que le coffre, sur les deux OS :
   une invite par mise à jour et par binaire, un modèle, un verrou (I2). Coût : moins lisible dans Seahorse.
6. **Origines S3** : lier au triplet (endpoint canonique, région, bucket) ; virtual-host et path-style en sont deux
   formes. SigV4 n'envoie pas la clé secrète : le risque est l'usage des droits sur un autre bucket (écrasement), donc
   changer bucket, région ou endpoint = même confirmation qu'une origine.
7. **Entropie** : pas critère principal. Toute valeur d'en-tête, d'argument, de paramètre ou de corps est secrète sauf
   liste blanche (`Accept`, `Content-Type`, `User-Agent`, gabarits purs), montrée masquée et reclassable ; avec `--yes`,
   tout le reste va au magasin. Entropie (≥ 3,5 bits/car. sur ≥ 16) pour les segments de chemin seulement (I10).
8. **`PR_SET_DUMPABLE(0)`** : le garder dans `kropa-app` en release et dans `kropa` quand il tient un secret : il
   interdit aussi `ptrace` et `/proc/<pid>/mem` au même utilisateur non root [8]. Coût faible (ni core dump ni gdb sans
   root, aucun rapport de plantage automatique). Désactivé sous `debug_assertions`, jamais par variable. macOS : le
   runtime renforcé bloque déjà l'attache.

## Non vérifié

Invites Keychain réelles (15, 26, 27) ; `LockedHint` par bureau ; échec d'un service sans DH ; proxy par défaut de
`reqwest` 0.13 ; audit RustSec (crates de stockage pas encore ajoutées). Git : `.env.example` suivi (prévu, non lu).

## Sources (11/11/2026)

[1] https://github.com/C2SP/C2SP/blob/main/age.md — scrypt : stanza seule, « SHOULD apply an upper limit », exemple 18.
[2] https://docs.rs/age/0.12.1/age/scrypt/ — `Recipient` ≈ 1 s ; `set_max_work_factor`, > 22 : heures et Gio.
[3] `zbus-secret-service-keyring-store` 1.0.1, `src/service.rs` (`connect(EncryptionType::Dh)`, `unlock_all`).
[4] `apple-native-keyring-store` 1.0.2 `src/keychain.rs` ; `security-framework` 3.7.0 `src/os/macos/passwords.rs`.
[5] https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp — `partitionIdForProcess`.
[6] https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/sys/fs/unix.rs — `fsync` = `F_FULLFSYNC` (Apple).
[7] `object_store` 0.14.2, `src/aws/builder.rs` (`InstanceCredentialProvider`, `with_http_connector`).
[8] man7.org : `unix(7)`, `user_namespaces(7)` (identifiants traduits), `PR_SET_DUMPABLE(2const)`, `ptrace(2)`.
