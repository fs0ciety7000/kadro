# Modèle de sécurité des secrets — Kropa

> Tâche S.1, rédigée par `architect` le 11/11/2026. **À revoir** par `security-reviewer` (S.2), puis validation de
> l'utilisateur (S.3). Prime sur `uploads.md` § 4 et § 7 (Argon2id/XChaCha remplacé par `age`, vidéo retirée).
> Aucun code de stockage (1.8, 2.8, 4.7) avant validation. « Doit » = exigence testable ; « non vérifié » = à tester.

## 1. Inventaire et transit

| Secret | Destination | Forme | Saisi par |
|---|---|---|---|
| Jeton Zipline | HTTP (préréglage) | en-tête `Authorization` | TTY, stdin, dialogue, import `.sxcu` |
| Clé d'accès + clé secrète S3 | `object_store` | identifiants statiques (jamais `from_env`, IMDS, profils AWS) | TTY, stdin, dialogue |
| Mot de passe SFTP ; phrase de passe de clé privée | `russh` | mot de passe ; la clé reste un fichier de l'utilisateur, jamais copiée | TTY, stdin, dialogue |
| En-têtes, arguments, paramètres d'une destination HTTP | `kropa-upload` | `{secret:nom}` dans un gabarit de requête | TTY, stdin, dialogue, import `.sxcu` |
| Phrase de passe du coffre `age` | coffre local | jamais stockée | TTY, `--password-fd`, dialogue |

Hors inventaire (sensibles mais non secrets) : URL de suppression (historique oui, journaux non), `SSH_AUTH_SOCK`
(chemin d'agent lu tel quel, pas un secret), clé privée de signature macOS (secret de CI, hors application).

**Transit autorisé, et lui seul** : saisie (TTY masqué, stdin, descripteur, dialogue natif) → processus `kropa` ou
`kropa-app` → magasin (§ 2, § 3). À l'envoi : magasin → mémoire `Zeroizing` → requête TLS vers une origine autorisée
(§ 4). Entre CLI et résident : socket Unix JSON-RPC (2.1), fichier `0600` dans `$XDG_RUNTIME_DIR` (Linux) ou dossier
`0700` propre à l'utilisateur (macOS), avec contrôle de l'UID pair (`SO_PEERCRED` / `getpeereid`). Seules méthodes qui
portent une valeur : `secrets.set` et `secrets.unlock` ; aucune méthode ne **renvoie** une valeur (pas de
`secrets.get` côté IPC). **Jamais** : argument de commande, variable d'environnement, fichier de config, base SQLite,
presse-papiers, journal, notification, sortie `--json`.

## 2. Stockage principal : trousseau système

Crate **`keyring` 4.2.0** (2026-08-29) sur **`keyring-core` 1.0.1**, magasins choisis à la compilation :
`zbus-secret-service-keyring-store` 1.0.1 (Linux, Rust pur) et `apple-native-keyring-store` 1.0.2 (macOS, module
`keychain`). Versions relues sur crates.io le 11/11/2026 [1][2][3][4]. Autres magasins de l'écosystème (keyutils,
`db-keystore`, dbus/libdbus) : **non retenus**. Clé d'une entrée : service `kropa`, compte `<profil>/<nom>`.

### 2.1 Linux : Secret Service (gnome-keyring, oo7, ksecretd de Plasma 6, KeePassXC)

- **Attributs** posés par le magasin : `service`, `username` (obligatoires), `target` (facultatif), libellé
  `keyring:{user}@{service}` sauf modificateur [3]. Kropa fixe le libellé « Kropa — <profil>/<nom> » (lisible dans
  Seahorse/KWallet) et **n'utilise pas** `target`, qui crée une collection séparée [3].
- **Collection** : alias `default` (la collection « login », déverrouillée à l'ouverture de session). Si elle est
  **verrouillée**, on demande son déverrouillage par le prompt du service (`Unlock` + `Prompt`) ; un refus ou un prompt
  fermé = statut `keyring_locked`, jamais repli implicite vers le coffre. En CLI non interactive (pas de session
  graphique pour afficher le prompt) : échec explicite, code de sortie dédié.
- **Absence de démon** (wlroots, X11 minimal, SSH) : pas de nom `org.freedesktop.secrets` sur le bus de session et pas
  d'activation D-Bus → statut `keyring_unavailable`. Kropa **ne lance pas** de démon et ne démarre pas de session
  D-Bus. Il propose le coffre (§ 3) par un message explicite.
- **Session de transport** : le magasin dépend de fonctions de `secret-service` qui fixent runtime et crypto (quatre
  options exclusives, non nommées dans la doc relue) [3]. Exiger la variante **chiffrée** (`dh-ietf1024-sha256-aes128-cbc-pkcs7`)
  si elle est offerte ; sinon le dire (question 2). Le bus de session reste lisible par tout processus du même
  utilisateur : hors périmètre (§ 7).
- Flatpak : hors distribution v1 (`decisions.md`) ; le portail Secret ne serait qu'une clé maître pour notre fichier.

### 2.2 macOS : Keychain à fichier (login)

- Module `keychain` du magasin Apple = trousseau « legacy » à fichier, chiffré, accessible à toute app [4]. Le module
  `protected` (data protection) est réservé aux apps sandboxées et à des entitlements de profil de provisionnement [4]
  [5] : **non utilisable** sans compte Developer, donc non activé.
- Élément : mot de passe générique, service `kropa`, compte `<profil>/<nom>`. Contrôle d'accès en deux couches
  (sources Apple partielles, sources tierces pour la seconde) :
  1. **ACL des applications de confiance**, désignées par leur *Designated Requirement* (DR) [6]. Avec un certificat
     auto-signé stable, la DR est `identifier "…" and certificate leaf = H"…"` et survit aux mises à jour ; en ad hoc,
     la DR est liée à la version et change à chaque build [6].
  2. **Liste de partitions** (`partition_id`, macOS ≥ 10.12) : `apple:`, `apple-tool:`, `teamid:<id>` ou `cdhash:<…>`.
     Un certificat auto-signé n'a **pas de Team ID Apple** ; un développeur signé par une CA non Apple a constaté
     qu'après mise à jour un nouveau `cdhash` s'ajoute à la partition, **avec une invite**, malgré une DR stable [7][8].
- **Conséquence (à vérifier en 1.6 sur le Mac)** : le certificat auto-signé protège probablement les permissions TCC,
  mais **pas forcément** l'absence d'invite Keychain à chaque mise à jour, contrairement à ce qu'affirme
  `platform-macos.md` § 4.4. Hypothèse de travail : une invite « Kropa veut utiliser… » par élément après chaque mise
  à jour, en ad hoc **et** en auto-signé ; « Toujours autoriser » ajoute le nouveau `cdhash`. Mitigations : regrouper
  les secrets d'un profil dans **un seul élément** (moins d'invites), mettre à jour en place (`SecItemUpdate`) au lieu de
  supprimer/recréer (qui réinitialise l'ACL) [8], et l'annoncer dans les notes de version. Jamais d'ajout de
  `unsigned:` ni de `security set-generic-password-partition-list` par Kropa.
- Invite refusée = statut `keychain_denied`, la tâche passe `waiting_unlock`, jamais repli implicite.

### 2.3 Format de la valeur stockée (trousseau et coffre)

Une entrée = un document JSON versionné, sérialisé dans un tampon `Zeroizing` :
`{"v":1, "origins":["https://img.example.org:443"], "created":"…", "value":"…"}`. Les **origines autorisées** (§ 4)
font partie de la valeur protégée : modifier la config TOML en clair ne les change pas.

## 3. Repli : coffre `age` à phrase de passe

- **Crate `age` 0.12.1** (2026-07-14), recipient `scrypt` seul (la spécification impose qu'un stanza scrypt soit le
  **seul** stanza du fichier) [9][10]. Pas de format maison ; déchiffrable en secours par `age -d` ou `rage -d`.
- **Format** : fichier age v1 binaire (pas d'armure) ; en-tête `age-encryption.org/v1`, stanza
  `scrypt <sel 16 o> <log2 N>`, MAC HMAC-SHA-256 ; charge utile STREAM ChaCha20-Poly1305 par blocs de 64 Kio [10]. Le
  clair est un document JSON : `{"v":1, "entries":{"<profil>/<nom>": <valeur § 2.3>}}`.
- **Facteur de travail** : scrypt r = 8, p = 1 [10]. À l'écriture, `log2 N` **fixé à 18** (≈ 256 Mio, valeur de
  l'exemple de la spec) plutôt que la calibration par défaut du crate (≈ 1 s sur la machine) [9], pour qu'un coffre créé
  sur une machine rapide s'ouvre sur une lente ; relevable par réglage. À la lecture,
  `set_max_work_factor(20)` : le défaut du crate vise ≈ 16 s et la doc prévient qu'au-delà de 22 un fichier piégé peut
  coûter des heures et des dizaines de Gio [9]. Facteur lu < 16 → avertissement « coffre faible, à réécrire ».
- **Emplacement** : `$XDG_DATA_HOME/kropa/secrets.age` (Linux, défaut `~/.local/share`) ;
  `~/Library/Application Support/<App ID>/secrets.age` (macOS). Hors dossier de config, pour qu'un dépôt de dotfiles
  ne l'emporte pas par défaut. Dossier `0700`, fichier **`0600`** ; à l'ouverture, refus si le fichier appartient à un
  autre UID ou est lisible par le groupe ou les autres (`kropa doctor` explique comment corriger).
- **Écriture atomique** : temporaire `secrets.age.<aléa>.tmp` créé `O_CREAT|O_EXCL` en `0600` dans le **même dossier**,
  écriture, `fsync` du fichier, `rename` sur la cible, `fsync` du dossier. Verrou consultatif (`flock`) pendant
  lecture-modification-écriture : CLI et résident peuvent écrire. Chaque écriture tire un nouveau sel et une nouvelle
  clé de fichier (assuré par `age`).
- **Mémoire** : phrase de passe, clair JSON et valeurs dans `zeroize::Zeroizing` (`zeroize` 1.9.1) ou
  `age::secrecy::SecretString`, jamais `String` nu ; `mlock` au mieux, **non garanti** (swap, vidage mémoire) : dit dans
  la doc. Réécrire le coffre exige la phrase de passe en mémoire pendant la session (limite connue de l'option A,
  `uploads.md` § 4.2) : elle est effacée au verrouillage.
- **Déverrouillage, deux niveaux** :
  1. *Par processus* (lot 2, 2.8) : sans résident, chaque `kropa` qui a besoin d'un secret demande la phrase de passe
     (TTY masqué), l'utilise, efface tout à la sortie.
  2. *Par session du résident* (lot 4, 4.7) : `kropa secrets unlock` (ou dialogue) transmet la phrase de passe au
     résident par l'IPC ; celui-ci garde le coffre ouvert **15 min d'inactivité** (réglable, ou « jusqu'au verrouillage
     de session »), puis efface. Verrouillage immédiat sur verrouillage d'écran (logind `Lock`/`PrepareForSleep` ;
     `com.apple.screenIsLocked` sur macOS) quand c'est détectable ; sinon le dire dans `kropa secrets status`.
     `kropa secrets lock` verrouille à la demande.
- **`--password-fd N`** : lit la phrase de passe sur le descripteur `N` jusqu'à EOF ou `\n`, puis le ferme ; refus si
  `N` vaut 0 alors que stdin porte aussi un fichier à envoyer. Pas de `--password`, pas de `KROPA_PASSWORD`.
- **Non interactif** (pas de TTY, pas de `--password-fd`, pas de résident déverrouillé) : aucun blocage ni attente,
  échec immédiat avec code dédié et message « coffre verrouillé : `kropa secrets unlock` ou `--password-fd` ». Dans la
  file du résident, la tâche passe `waiting_unlock`, sans échec.
- **Mauvaise phrase de passe** : message unique « phrase de passe incorrecte ou fichier altéré » (age ne distingue pas),
  délai croissant après 3 essais dans le résident. Phrase perdue = secrets perdus (dit à la création, avec la commande
  de ressaisie).

### 3.1 Choix du magasin, bascule jamais silencieuse

- Ordre : trousseau s'il répond ; coffre seulement si l'utilisateur le **choisit** (`kropa secrets use vault`, ou
  réglage), y compris quand le trousseau est absent. Le choix est écrit dans la config (`secrets.store = "keyring" |
  "vault"`) ; il ne change jamais tout seul.
- Trousseau devenu indisponible, verrouillé ou refusé alors que `store = "keyring"` : erreur explicite, **pas** de
  lecture ni d'écriture dans le coffre. Inversement, coffre choisi : jamais d'écriture au trousseau.
- Migration explicite seulement : `kropa secrets migrate --to vault|keyring` (copie, vérifie par relecture, puis
  supprime la source après confirmation).
- `kropa secrets status [--json]` : magasin, état (`unlocked`, `locked`, `unavailable`, `denied`), facteur scrypt du
  coffre, nombre d'entrées, **jamais** de valeur ni de liste de valeurs.

## 4. Liaison secret ↔ origines et résolution

- **Origine** = schéma + hôte (IDNA, minuscules) + port explicite. Fixée à la création d'après l'URL du profil
  (`kropa dest add`, import `.sxcu`), stockée **dans** la valeur protégée (§ 2.3).
- **Références** : la config TOML ne contient que `[destination.secrets] token = { ref = "<profil>/<nom>" }` et des
  gabarits `{secret:token}`. Résolution au **dernier moment**, juste avant l'envoi, dans `kropa-upload` :
  1. la requête est construite sans secret ; son origine est calculée ;
  2. chaque `{secret:x}` exige que l'origine figure dans `origins` de `x`, sinon refus **avant toute connexion** ;
  3. `{secret:}` n'est permis que dans en-têtes, paramètres de requête, champs de formulaire et corps de requête ;
     **interdit** dans l'URL de base (hôte, chemin), le nom de fichier et **tous** les gabarits de réponse (`url`,
     `thumbnail_url`, `deletion_url`, `error`) — vérifié au chargement de la config, pas à l'envoi.
- **Changement d'URL** d'un profil qui utilise un secret : l'autorisation ne suit pas. Confirmation interactive qui
  montre l'ancienne et la nouvelle origine ; en non interactif, refus. Origine ajoutée par `kropa secrets allow`.
- **HTTPS obligatoire** dès qu'un secret est résolu, exceptions `localhost`, `127.0.0.1`, `::1` (tests). Vérification
  TLS rustls par défaut, aucune option pour l'ignorer en v1.
- **Redirections** : client `reqwest` unique avec politique maison : `none` ou `same-origin` (défaut), 5 sauts au
  plus ; une redirection vers une autre origine **échoue** (même sans secret : pas de suivi « en-têtes retirés »). HTTPS → HTTP
  interdit.
- **Réponse** : corps lu en flux et **coupé à 1 Mio** (erreur au-delà), délais de connexion et totaux, annulation.
  L'URL extraite doit être `http(s)` sans caractère de contrôle avant toute copie.
- S3 et SFTP : l'origine est l'endpoint S3 (ou région AWS résolue) et `sftp://hôte:port` ; SFTP impose en plus
  `known_hosts` strict (4.9). Un secret S3 ne sert jamais à une autre destination.

## 5. Import `.sxcu`

- Le fichier est **lu comme données** : JSON borné (≤ 64 Kio), schéma ShareX ≥ 13.7.1 ; rien n'est exécuté, aucun
  `{inputbox:}` ni `{select:}` résolu sans l'utilisateur, aucune requête réseau pendant l'import.
- **Extraction** : en-têtes connus (`Authorization`, `x-api-key`, `x-zipline-*`, `Cookie`…), `Arguments`/`Parameters`
  nommés `key`, `token`, `password`, `secret`, `apikey`…, segments de `RequestURL` ou valeurs à forte entropie. Chaque
  valeur est montrée **masquée** (4 derniers caractères au plus), rangée dans le magasin choisi avec l'origine de
  `RequestURL`, remplacée par `{secret:…}` dans le TOML.
- **Confirmation de l'hôte** : résumé (origine, HTTPS ou non, champs, secrets détectés), création du profil seulement
  après accord ; en non interactif, `--yes` **et** `--expect-host <hôte>` exigés, sinon refus. Un `.sxcu` en `http://`
  avec secret est refusé (sauf loopback).
- Un `.sxcu` ne peut **pas** réutiliser un secret existant : ses références pointent toujours vers des entrées neuves.
  Le fichier d'origine n'est pas recopié ; Kropa invite à le supprimer (il contient le jeton en clair).

## 6. Journaux, erreurs, interfaces

- Types : `SecretValue` (et tout type qui en contient) implémente `Debug` et `Display` par `***` (`secrecy` ou type
  maison), pas de `Serialize` ; `Clone` limité. Lint maison ou test qui échoue si un type secret dérive `Debug`.
- Couche `tracing` d'expurgation : en-têtes en **liste blanche** (`content-type`, `content-length`, `retry-after`,
  `location` sans requête) ; `Authorization`, `Cookie`, `Set-Cookie`, `x-api-key` et champs marqués secrets → `***`.
  Passe finale : toute valeur de secret chargée est remplacée par `***` dans messages, erreurs, extraits de réponse
  (500 caractères max) et sortie `--json`. Jamais de corps de requête journalisé.
- Erreurs : `kropa-upload` renvoie des erreurs typées sans valeur ; les erreurs de `reqwest` sont reformulées (leur URL
  peut porter un paramètre secret) ; les messages Fluent ne reçoivent jamais de valeur.
- **Pas en argument** : `kropa dest add --token` n'existe pas ; un argument ressemblant à un secret (`--header
  "Authorization: …"`) est refusé avec renvoi vers stdin/TTY. **Pas en variable d'environnement** : aucune lecture de
  `KROPA_*`, `AWS_*`, `ZIPLINE_*` ; `object_store` construit sans `from_env`.
- Rapport de bug et `kropa doctor` : config avec références seulement, aucun contenu de magasin.
- Fichiers d'échec et core dumps : `prctl(PR_SET_DUMPABLE, 0)` dans le résident Linux (au mieux), `RLIMIT_CORE = 0`.

## 7. Modèle de menaces

Hors périmètre, dit dans la doc : processus malveillant du **même utilisateur** (il lit la mémoire, le bus de session,
le trousseau déverrouillé), root, machine compromise, serveur de destination lui-même malveillant pour son propre jeton.

| Menace | Mitigation | Test qui le prouve |
|---|---|---|
| Secret lu sur disque (sauvegarde, dotfiles) | Trousseau ou coffre `age` ; config avec références seulement | `kropa-store` : grep des valeurs connues dans config, SQLite, coffre (chiffré) → absentes |
| Coffre lisible par un autre utilisateur | `0600`/`0700`, refus d'ouvrir sinon | test : coffre `0644` → erreur dédiée ; temporaire créé `0600` |
| Coffre tronqué par un plantage | Écriture atomique + `fsync` + `flock` | test d'injection de panne entre écriture et `rename` : ancien coffre intact |
| Attaque hors ligne du coffre | scrypt `log2 N` = 18, relevable | test : facteur lu dans l'en-tête = réglage ; < 16 → avertissement |
| Fichier `age` piégé (facteur énorme) | `set_max_work_factor(20)` | fixture `log2 N` = 30 → refus immédiat (< 1 s) |
| Bascule silencieuse vers un magasin moins sûr | `secrets.store` explicite, pas de repli | tests : bus sans Secret Service + `store = keyring` → erreur, coffre non créé |
| `.sxcu`/TOML malveillant qui vise un jeton existant | Origines dans la valeur protégée, refus avant connexion | `wiremock` : `{secret:x}` vers origine non autorisée → zéro requête reçue |
| Redirection vers un autre hôte | `same-origin`, 5 sauts, pas de HTTPS→HTTP | `wiremock` : 302 vers autre port/hôte → échec, second serveur sans requête |
| Interception réseau | HTTPS obligatoire avec secret, TLS vérifié | test : `http://` non loopback + secret → refus au chargement |
| Secret recopié dans le lien, l'historique, le presse-papiers | `{secret:}` interdit en réponse et dans le nom | test de config : `url = "{secret:t}"` → erreur de validation |
| Serveur qui renvoie le jeton | Expurgation des valeurs connues | test 1.9 : réponse d'erreur contenant le jeton → journaux et `--json` avec `***` |
| Réponse énorme, serveur lent | 1 Mio, délais, annulation | `wiremock` : 2 Mio → erreur sans dépassement mémoire ; délai → `timeout` |
| Secret en argument (`ps`, historique du shell) | Pas d'option, refus des motifs | `trycmd` : `--header "Authorization: …"` → refus |
| Secret en variable d'environnement | Aucune lecture | test : `AWS_SECRET_ACCESS_KEY` défini → ignoré ; « zéro requête » (1.11) |
| Fuite par `Debug`/panique | `Debug` masqué, pas de `Serialize` | test : `format!("{:?}")` d'un profil chargé ne contient pas la valeur |
| Autre utilisateur sur la socket IPC | `0600`, contrôle d'UID, pas de `secrets.get` | test : connexion d'un UID différent (namespace) → refus |
| Coffre laissé ouvert | 15 min, verrouillage d'écran, `secrets lock` | horloge injectable : 15 min + 1 s → `locked`, mémoire effacée (inspection du tampon) |
| Invites Keychain à chaque mise à jour | Un élément par profil, `SecItemUpdate`, doc | manuel 1.6 : deux builds signés → nombre d'invites relevé |
| Usurpation d'hôte SFTP | `known_hosts` strict | test OpenSSH : clé changée → refus |
| `.sxcu` partagé avec le jeton de l'auteur | Détection, masquage, rangement, avertissement | fixture `.sxcu` Zipline → TOML sans valeur, entrée créée liée à l'origine |

## 8. Questions ouvertes pour la revue

1. **Keychain et auto-signé** : la partition `cdhash:` provoque-t-elle une invite à chaque mise à jour malgré une DR
   stable [7][8] ? Si oui, faut-il proposer le coffre par défaut sur macOS sans compte Developer ? Test en 1.6.
2. **Session Secret Service** : quelle fonction de `secret-service` choisir (runtime tokio + crypto Rust) et le
   chiffrement de transport est-il réellement négocié ? Exiger `dh-ietf1024…` ou accepter `plain` (bus local) ?
3. **Facteur scrypt** : 18 fixe (portable) ou calibration ≈ 1 s du crate (plus fort sur machine rapide) ? Plafond de
   lecture 20 suffisant ?
4. **Phrase de passe en mémoire** dans le résident pour réécrire le coffre : acceptable, ou forcer un nouveau
   déverrouillage pour toute écriture (aucune phrase gardée, seulement les valeurs) ?
5. **Granularité** : un élément de trousseau par secret (simple) ou par profil (moins d'invites macOS) ?
6. **Origines S3** : une région AWS implicite (`s3.<région>.amazonaws.com` et virtual-host `<bucket>.s3…`) compte-t-elle
   comme une seule origine autorisée ?
7. Détection par **entropie** à l'import `.sxcu` : seuil, et risque de faux négatifs sur jetons courts.
8. `prctl(PR_SET_DUMPABLE, 0)` gêne-t-il le débogage et les rapports de plantage (apport réel faible vu § 7) ?

## Sources

[1] https://crates.io/crates/keyring (4.2.0) et https://docs.rs/keyring/4.2.0/keyring/ — magasins disponibles.
[2] https://crates.io/crates/keyring-core (1.0.1).
[3] https://docs.rs/zbus-secret-service-keyring-store/1.0.1/ — attributs, collection par défaut, `target`, fonctions.
[4] https://docs.rs/apple-native-keyring-store/1.0.2/ — modules `keychain` et `protected`.
[5] https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
[6] https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements
[7] https://developer.apple.com/forums/thread/98484 — invite après mise à jour, CA non Apple, nouveau `cdhash`.
[8] https://developer.apple.com/forums/thread/746931 ; rapports tiers sur la liste de partitions et la recréation
    d'éléments : https://claudeissues.com/issue/53501-remote-daemon-claude-remote-server-ships-adhoc-codesigned-breaks-keychain-partit
[9] https://docs.rs/age/0.12.1/age/scrypt/struct.Recipient.html et `struct.Identity.html` — calibration ≈ 1 s,
    plafond par défaut ≈ 16 s, avertissement au-delà de 22.
[10] https://github.com/C2SP/C2SP/blob/main/age.md — stanza scrypt, r = 8, p = 1, stanza unique, STREAM 64 Kio.
Versions relevées sur l'API crates.io le 11/11/2026 : `age` 0.12.1, `zeroize` 1.9.1, `secrecy` 0.10.3.
