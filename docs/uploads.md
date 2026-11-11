# Kropa : envoi et intégrations

> Proposition d'équipe du 11/11/2026. En cas de contradiction, docs/decisions.md prévaut.

Équipe « Envoi et intégrations ». Proposition à valider, aucun code écrit. État vérifié en ligne le 11/11/2026
(sources en fin de document). Principes rappelés : local d'abord, aucune requête réseau sans destination configurée,
zéro télémétrie, aucune destination imposée, secrets jamais en clair.

**Valeur par défaut : aucun envoi.** Une installation neuve n'a aucune destination. L'envoi n'existe qu'après la
création explicite d'un profil.

## 0. Socle technique (crates)

| Besoin | Options comparées | Choix | Raison |
|---|---|---|---|
| Client HTTP | `reqwest` 0.13.5 (async, rustls par défaut, MIT/Apache) · `ureq` 3.5.1 (synchrone, MIT/Apache) | **reqwest** | Le flux vers un fichier, le multipart, les annulations et la file concurrente sont plus simples en async. `object_store` s'appuie déjà dessus, donc pas de deuxième pile HTTP. |
| S3 | `aws-sdk-s3` 1.159 (Apache-2.0, très lourd) · `rust-s3` 0.38 (MIT, un seul mainteneur) · `object_store` 0.14.2 (Apache Arrow, MIT/Apache) | **object_store** (feature `aws`) | Il est léger comparé au SDK AWS. Il offre une API multipart bas niveau (`MultipartStore` : identifiant d'upload, `put_part`, `complete`) qui permet de reprendre après un redémarrage. Il accepte un endpoint personnalisé et le style « path ». Il évite aussi les sommes CRC32 par défaut du SDK AWS (depuis la v1.69), qui ont cassé R2 et Backblaze en 2025. |
| SFTP | `russh` 0.65.1 + `russh-sftp` 3.0.1 (Rust pur, Apache-2.0, tokio) · `ssh2` 0.9.7 (libssh2 en C, OpenSSL sous Linux, synchrone) | **russh + russh-sftp** | Pas de dépendance C ni OpenSSL à empaqueter sur macOS. Async, cohérent avec la file. Projet actif (versions en novembre 2026). Licence Apache-2.0 : voir § 8. |
| JSONPath | `jsonpath-rust` 1.0.12 (RFC 9535, actif) · `serde_json_path` 0.7.2 (dernière version 02/2025) | **jsonpath-rust** | Actif et conforme à la RFC. ShareX utilise le `SelectToken` de Newtonsoft, qui en est proche pour les chemins usuels (`files[0].url`). |
| Trousseau | `keyring` 4.2 (+ `keyring-core` 1.0) · `secret-service` 5.2 / `security-framework` 3.7 en direct | **keyring 4** avec `zbus-secret-service-keyring-store` (Linux) et `apple-native-keyring-store` (macOS) | Une seule API, et des magasins choisis à la compilation. zbus est en Rust pur, alors que la variante dbus dépend de libdbus. |
| Coffre de repli | `age` 0.12.1 (phrase de passe via scrypt) · ~~Argon2id (`argon2` 0.6) + XChaCha20-Poly1305 (`chacha20poly1305` 0.11)~~ | ~~Argon2id + XChaCha20-Poly1305~~ → **`age`** (décision du 11/11/2026) | Voir § 4.2 et `docs/security/secrets.md`. |
| File | `rusqlite` 0.42 (SQLite) · un fichier JSON par tâche | **SQLite** | Voir § 5. |
| Métadonnées | `img-parts` 0.4.2 (segments/chunks JPEG, PNG, WebP) · `little_exif` 0.6.23 (édition EXIF) · `rexiv2` (GPL-3, C++) | **img-parts** (~~+ FFmpeg pour la vidéo~~ : vidéo retirée, décision du 11/11/2026) | Voir § 6. `rexiv2` est écarté (licence et C++). |

Les versions sont celles de crates.io au 11/11/2026. On les revérifie et on lance `cargo audit` (dépôt RustSec) au
moment de les ajouter.

## 1. Les quatre destinations intégrées

> Décision du 11/11/2026 : Imgur est **retiré des destinations intégrées** (modèle de destination personnalisée, § 1.4).
> Restent trois destinations intégrées : Zipline, S3 et compatibles, SFTP.

### 1.1 Zipline (v4, actuelle : 4.8.3, MIT)

- **Protocole** : `POST {base}/api/upload`, multipart/form-data (champ `file`). Lu dans le code source v4.8.3.
- **Authentification** : jeton utilisateur, brut, dans `Authorization` (sans `Bearer`).
- **En-têtes utiles** : `x-zipline-format`, `x-zipline-deletes-at`, `x-zipline-original-name`, `x-zipline-folder`,
  `x-zipline-password`, `x-zipline-max-views`, `x-zipline-filename`, `x-zipline-domain`, `x-zipline-no-json`.
- **Réponse** : JSON `{ files: [{ id, name, type, url, pending?, removedGps? }], deletesAt? }`. L'URL publique est
  dans `files[0].url`. `pending: true` signifie un traitement asynchrone côté serveur : l'URL est valide, mais le
  fichier n'est pas encore servi.
- **Envoi par morceaux** : `POST /api/upload/partial`. Morceaux séquentiels avec `Content-Range: bytes a-b/total`,
  `x-zipline-p-filename`, `x-zipline-p-content-type`, `x-zipline-p-content-length` (= total) et
  `x-zipline-p-lastchunk: true` sur le dernier. Chaque morceau intermédiaire renvoie `partialSuccess` et un
  `partialToken` à usage unique, valable 30 min, à renvoyer en `x-zipline-p-token`. Limites serveur par défaut :
  `chunks.size` 25 Mo, `chunks.max` 95 Mo, lisibles sans authentification via `GET /api/server/public`.
  Kropa lit ce point d'accès au test du profil et passe en morceaux au-delà de `chunks.size`.
- **Particularités** : l'état partiel vit **en mémoire** du serveur. Un redémarrage de Zipline, ou plus de 30 min
  d'interruption, oblige à recommencer depuis zéro. Pas d'URL de suppression dans la réponse. La suppression se
  fait par l'API utilisateur avec le jeton (v2 de Kropa).
- **Zipline v3** : non ciblée, possible via destination personnalisée (non testé).

### 1.2 S3 et compatibles

- **Protocole** : S3 signé SigV4 (`PutObject` sous le seuil, multipart au-dessus). Paramètres : endpoint, région,
  bucket, préfixe de clé, style path/virtual-host, `public_base_url`.
- **Authentification** : clé d'accès et clé secrète, toutes deux rangées dans le trousseau.
- **Envoi par morceaux** : multipart, morceaux de 8 Mio (minimum S3 et R2 : 5 Mio, sauf le dernier ; R2 exige des
  morceaux de taille égale ; 10 000 morceaux au maximum). Kropa persiste l'`UploadId` et les ETag des morceaux finis
  pour reprendre. En cas d'abandon, il appelle `AbortMultipartUpload`. Sinon, R2 purge seul après 7 jours, et
  ailleurs c'est la règle de cycle de vie du bucket.
- **URL publique** : S3 ne la renvoie pas. Kropa la **construit** : `public_base_url + clé` (CDN, domaine R2,
  site web Garage). En option, une URL présignée de durée bornée (≤ 7 j) pour un bucket privé.
- **Particularités par service** :
  - AWS S3 : référence.
  - Cloudflare R2 : pas d'ACL objet ; l'accès public se règle au niveau du bucket ou du domaine.
  - Garage : multipart et présignature implémentés ; pas d'ACL ni de politique de bucket, publication par « website ».
  - Backblaze B2 : pas d'ACL objet ; `PutBucketAcl` limité à private/public-read.
  - MinIO : édition communautaire en maintenance depuis le 03/12/2025, dépôt archivé (avril 2026). On le
    garde comme cible de test, sans le mettre en avant.
  - **Aucun envoi d'en-tête `x-amz-acl` par défaut** : beaucoup de services le refusent. Option explicite seulement.

### 1.3 SFTP

- **Protocole** : SSH puis sous-système SFTP (russh-sftp).
- **Authentification**, dans l'ordre : agent SSH (`SSH_AUTH_SOCK`), clé privée (chemin en config, phrase de passe
  dans le trousseau), mot de passe (trousseau).
- **Clé d'hôte obligatoire** : lecture de `~/.ssh/known_hosts` et d'un `known_hosts` propre à Kropa. Hôte inconnu :
  l'interface affiche l'empreinte SHA-256 et demande confirmation. En CLI, on passe `--accept-host-key SHA256:…`
  explicitement, sinon c'est un échec. Une clé changée donne un refus net, jamais un contournement.
- **Envoi par morceaux et reprise** : écriture dans `nom.part`. En reprise, on lit la taille distante (`stat`) et on
  reprend à cet offset, puis `rename` atomique.
- **URL publique** : non fournie par le protocole. Elle vient d'un modèle `public_url = "https://f.example.com/{remote_name}"`.
  Sans modèle, Kropa copie le chemin distant et le signale.
- **Particularités** : pas de lecture de `~/.ssh/config` en v1. `russh-config` existe mais il est jeune, c'est une
  question ouverte.

### 1.4 Imgur : ~~intégrée, mais dégradée et à requalifier~~ modèle de destination personnalisée

> Tranché le 11/11/2026 : **option B retenue**. Imgur n'est pas une destination intégrée ; il est fourni comme modèle
> de destination personnalisée (§ 2). L'analyse ci-dessous est conservée comme justification.

- **Protocole** : `POST https://api.imgur.com/3/image` (images) ou `/3/upload` (vidéo), multipart. Réponse
  `data.link` (URL publique) et `data.deletehash` (suppression : `DELETE /3/image/{deletehash}`). Pas d'envoi par
  morceaux.
- **Authentification** : anonyme avec `Authorization: Client-ID <id>`, ou OAuth 2 (jeton d'accès + jeton de
  renouvellement) pour envoyer sur un compte.
- **Constat vérifié (11/11/2026)** : `https://api.imgur.com/oauth2/addclient` redirige (301) vers la page d'accueil.
  **On ne peut plus enregistrer de nouvelle application.** Un billet de mars 2026 décrit ce même comportement, sans
  annonce officielle. `apidocs.imgur.com` n'a pas pu être consulté par l'outil, donc les limites citées
  (≈ 1 250 envois/jour/application, contrôle via `/3/credits`) viennent de sources secondaires, **à vérifier**.
  Imgur bloque en outre tout accès depuis le Royaume-Uni depuis le 30/09/2025.
- **Conséquence pour « aucun service imposé »** : Kropa **n'embarque aucun client_id**. Un identifiant partagé
  serait un service imposé, rattaché au projet, révocable à tout moment, et nous ne pouvons de toute façon pas en
  obtenir un. L'utilisateur fournit son propre `client_id`, s'il en possède un ancien, et le range dans le trousseau.
  C'est acceptable : rien n'est requis, rien n'est contacté sans configuration.
- **Option A** : garder Imgur intégrée avec un `client_id` fourni par l'utilisateur.
  **Option B** : la rétrograder en modèle de destination personnalisée fourni en exemple.
  **Recommandation : B**, puisque l'écrasante majorité des utilisateurs ne pourra pas l'utiliser. On garde
  l'intégration A seulement si l'enregistrement rouvre. ~~Question ouverte n° 1.~~ Résolu : B.

## 2. Destination personnalisée

### 2.1 Format : TOML ou JSON ?

| | TOML (dans `destinations.toml`) | JSON (format `.sxcu` natif) |
|---|---|---|
| Écriture à la main | lisible, commentaires possibles | pas de commentaires, échappements lourds |
| Cohérence | même format que la config Kropa (à confirmer avec l'équipe Architecture) | format à part |
| ShareX | conversion à l'import | lecture directe, mais sémantique ShareX (menus interactifs, secrets en clair) |

**Choix : TOML**, avec un import `.sxcu` qui **convertit** en TOML et n'exécute jamais un `.sxcu` tel quel. Le
langage de gabarits reprend la syntaxe ShareX `{fonction:arg|arg}`, pour que l'import soit quasi littéral et que les
utilisateurs de ShareX s'y retrouvent.

### 2.2 Exemple complet

```toml
[[destination]]
id      = "maison"                      # nom de profil, utilisé par la CLI
type    = "http"
label   = "Mon serveur d'images"
kinds   = ["image", "file"]             # types acceptés (ShareX : DestinationType) ; "video" retiré du produit

[destination.request]
method  = "POST"
url     = "https://img.example.org/api/upload"   # https obligatoire si un secret est utilisé (sauf loopback)
query   = { visibility = "unlisted" }
headers = { Authorization = "Bearer {secret:token}", Accept = "application/json" }
body    = "multipart"                   # multipart | form | json | xml | binary | none
file_field = "file"                     # multipart uniquement
fields  = { title = "{filename}" }      # arguments du corps
max_size = "100MiB"                     # refus local au-delà, avant tout réseau
timeout  = "120s"
redirects = "same-origin"               # none | same-origin (défaut) ; jamais cross-origin avec secret

[destination.secrets]
token = { ref = "kropa/maison/token" }  # référence : la valeur vit dans le trousseau ou le coffre

[destination.response]
url          = "{json:$.data.url}"
thumbnail_url = "{json:$.data.thumb}"
deletion_url = "https://img.example.org/del/{json:$.data.delete_token}"
error        = "{json:$.error.message}"
success_status = [200, 201]             # sinon : erreur, même si le JSON contient une URL
```

### 2.3 Extraction de l'URL et erreurs

- Fonctions de réponse : `{response}`, `{responseurl}`, `{header:nom}`, `{json:chemin}`, `{json:entrée|chemin}`,
  `{xml:xpath}`, `{regex:motif|groupe}`. Côté requête : `{filename}`, `{input}`, `{random:a|b}`, `{base64:…}`,
  `{secret:nom}` (propre à Kropa).
- Le chemin JSON est compris comme RFC 9535. Préfixe `$.` ajouté s'il manque, comme le fait ShareX.
- **Validation de l'URL extraite** : elle doit être `http(s)` et ne pas contenir de caractère de contrôle. Sinon, la
  tâche est en échec, et on ne copie jamais n'importe quoi dans le presse-papiers.
- **Erreurs** : statut hors `success_status`, gabarit `url` vide ou JSON invalide → échec, avec le message
  `error` extrait (sinon les 500 premiers caractères de la réponse, **expurgés**, § 4.6). Classification pour la
  file : 408, 429, 5xx et erreurs réseau = transitoires (nouvel essai, `Retry-After` respecté) ; autres 4xx =
  définitives.
- `kropa dest test <id>` envoie une image témoin de 1×1 et affiche statut, en-têtes filtrés et URL extraite.

### 2.4 Correspondance avec `.sxcu` (ShareX v22.4, schéma `Version` ≥ 13.7.1)

| Champ / syntaxe `.sxcu` | Import | Remarque |
|---|---|---|
| `Name`, `DestinationType` | oui | → `label`, `kinds` |
| `RequestMethod`, `RequestURL`, `Parameters` | oui | |
| `Headers` | oui, **avec tri des secrets** | voir § 4.4 |
| `Body` (MultipartFormData, FormURLEncoded, JSON, XML, Binary, None) | oui | `Data` (corps JSON/XML brut) → `body_template` |
| `Arguments`, `FileFormName` | oui | |
| `URL`, `ThumbnailURL`, `DeletionURL`, `ErrorMessage` | oui | |
| `{json:}`, `{xml:}`, `{regex:}`, `{header:}`, `{response}`, `{responseurl}`, `{filename}`, `{input}`, `{random:}`, `{base64:}` | oui | sémantique de Newtonsoft `SelectToken` ≈ RFC 9535 ; les écarts éventuels sont signalés à l'import |
| `{select:a\|b}`, `{inputbox:titre\|défaut}` | partiel | interface : boîte de choix ; CLI : `--var nom=valeur`, sinon échec (jamais de blocage silencieux) |
| `{outputbox:}` | non | sans objet (pas d'envoi de texte ni de raccourcisseur d'URL) |
| `DestinationType` = URLShortener, URLSharingService, TextUploader | non | hors périmètre v1, refus explicite |
| `Version` ≤ 12.3.1 | non | ShareX lui-même les refuse |
| ≤ 13.7.1 (`$json:…$`) | oui | migration de l'ancienne syntaxe, comme ShareX |

L'import affiche un résumé (hôte, champs, secrets détectés) et ne crée le profil qu'après confirmation.

## 3. Profils, défaut, choix à la volée

- **Plusieurs profils**, éventuellement du même type (deux buckets S3, Zipline perso et Zipline équipe). Identifiant
  court et stable.
- **Défaut** : `default_destination` absent = enregistrement local seulement. Un défaut par **type de média**
  (image, fichier ; ~~vidéo/GIF~~ retirés du produit) est possible.
- **Envoi automatique** : désactivé par défaut. Il s'active par profil ou par chaîne d'actions (équipe « Après la
  capture »).
- **CLI**, à harmoniser avec l'équipe CLI :
  - `kropa capture area --upload` (défaut), `--upload=maison`, `--no-upload`.
  - `kropa upload fichier.png [--to maison] [--copy url|markdown|html|none] [--json]`.
  - `kropa dest list | show | add | edit | test | remove | set-default | import x.sxcu | export`.
  - `kropa queue list | retry <id> | cancel <id> | pause | resume`.
  - Sortie `--json` stable (id de tâche, URL, URL de suppression) pour les scripts.
- Choix à la volée dans l'interface : menu sur la miniature flottante (« Envoyer vers… »).

## 4. Sécurité des secrets

### 4.1 Trousseau ou fichier chiffré

| Plateforme | Magasin principal | Repli |
|---|---|---|
| macOS | Keychain (login), via `apple-native-keyring-store` | coffre chiffré |
| Linux | Secret Service D-Bus (GNOME Keyring, KWallet / ksecretd de Plasma 6, KeePassXC) | coffre chiffré si pas de service sur le bus, ou sur refus de l'utilisateur |

Bascule **jamais silencieuse** : Kropa dit quel magasin il utilise (`kropa secrets status`). Si les deux existent,
c'est le trousseau, sauf si l'utilisateur force le coffre.

**Limite macOS (signature ad hoc)** : l'ACL d'un élément Keychain est liée à l'identité de code. Avec une signature
ad hoc, cette identité change à **chaque mise à jour** du binaire, et macOS redemandera « Autoriser l'accès » après
chaque version. On le documente ; c'est inévitable sans compte Developer.

### 4.2 Coffre de repli : algorithme

> Tranché le 11/11/2026 : **`age`** (option A, fichier chiffré par phrase de passe). Le format maison ci-dessous
> (option B) est **abandonné** ; voir `docs/security/secrets.md`. Le texte est conservé comme historique.

- **Option A, `age` en mode phrase de passe (scrypt)** : format standard, déchiffrable avec `age`/`rage` en cas de
  secours. Mais il faut garder la phrase de passe en mémoire pour réécrire le coffre, et scrypt est moins résistant
  aux GPU qu'Argon2id.
- ~~**Option B, Argon2id + XChaCha20-Poly1305** (RustCrypto) : on garde en mémoire la **clé dérivée**, pas le mot de
  passe. Nonce aléatoire de 192 bits à chaque réécriture, sans risque de collision.~~
- ~~**Choix : B**, format minimal et documenté : `magic | version | params Argon2 (m, t, p) | sel 16 o | nonce 24 o | chiffré`,
  l'en-tête entier servant de données associées (AAD). Paramètres : Argon2id, m = 64 Mio, t = 3, p = 1, au-dessus
  du minimum OWASP (19 Mio, t = 2) ; on calibre pour ≈ 0,5 s et on stocke les paramètres pour les relever plus tard.
  Fichier `0600` dans le dossier de configuration, écriture atomique (temporaire puis `rename`). **À faire relire par
  `security-reviewer`** : c'est de l'assemblage de primitives, pas une crypto maison, mais le format est à nous.~~
- **Retenu : `age`.** Fichier `0600` dans le dossier de configuration, écriture atomique (temporaire puis `rename`).
  Paramètres et gestion de la phrase de passe : `docs/security/secrets.md`.
- Mot de passe perdu : les secrets sont perdus. On les ressaisit (jetons révocables côté service). C'est dit à la
  création.

### 4.3 Déverrouillage et durée de session

- Déverrouillage par boîte de dialogue (interface), invite TTY (CLI, saisie masquée) ou `kropa secrets unlock`, qui
  déverrouille le processus résident pour une durée. **Jamais** par variable d'environnement ni argument de ligne de
  commande. Option pour les scripts : `--password-fd N`.
- Durée : réglable (15 min d'inactivité par défaut, ou « jusqu'au verrouillage de session »). Verrouillage immédiat
  sur verrouillage d'écran (logind `Lock` / notification macOS) quand c'est détectable.
- En mémoire : clé et secrets dans des tampons `zeroize`, et `mlock` au mieux (non garanti, le dire).
- Tâches en attente pendant que le coffre est verrouillé : elles restent dans la file, état « en attente de
  déverrouillage », sans échec.

### 4.4 Où vivent les références

- La config (`destinations.toml`) ne contient **que des références** : `{ ref = "kropa/<profil>/<nom>" }`. Dans le
  trousseau, service `kropa` et compte `<profil>/<nom>`. Dans le coffre, c'est une entrée du même nom.
- Chaque secret stocké porte sa **liste d'origines autorisées** (schéma + hôte + port), fixée à sa création à partir
  de l'URL du profil. Elle fait partie de la valeur protégée, pas de la config modifiable en clair.
- **Import `.sxcu`** : un `.sxcu` contient souvent des secrets **en clair** : `Authorization`, `x-api-key`, jeton
  Zipline, `Arguments` comme `key`/`token`/`password`, voire un jeton dans `RequestURL` ou `Parameters`. Kropa les
  détecte (noms d'en-têtes et d'arguments connus, entropie élevée), les montre masqués, les range dans le magasin
  lié à l'origine de `RequestURL` et ne garde que `{secret:…}` dans le TOML. Le `.sxcu` d'origine n'est pas recopié,
  et l'utilisateur est invité à le supprimer.

### 4.5 Contre l'exfiltration

Une config tierce (un `.sxcu` reçu, un TOML copié) ne doit pas pouvoir envoyer un secret existant ailleurs :

1. `{secret:x}` ne se résout que si l'origine de la requête figure dans les origines autorisées de `x`. Sinon, refus
   avant tout envoi.
2. Changer l'URL d'un profil qui utilise un secret ne transporte pas l'autorisation : une confirmation interactive
   est demandée, jamais en mode non interactif.
3. Redirections : jamais vers une autre origine avec un secret. Par défaut, mêmes origines et 5 sauts au plus.
4. Les secrets sont interdits dans les gabarits de **réponse** (`url`, `deletion_url`…) et dans le nom de fichier :
   cela empêche qu'un secret finisse dans le presse-papiers ou l'historique.
5. HTTPS obligatoire dès qu'un secret est utilisé (exception : `localhost` et `127.0.0.1`). Certificats vérifiés,
   et pas d'option « ignorer TLS » en v1.
6. Un profil importé n'envoie que la capture désignée : aucun accès à d'autres fichiers locaux.

### 4.6 Journalisation sans secret

- Couche `tracing` d'expurgation : en-têtes journalisés sur liste blanche (`content-type`, `content-length`,
  `retry-after`…), valeurs de `Authorization` et `Cookie` et des paramètres marqués secrets remplacées par `***`.
- Passage final : toute occurrence d'une valeur de secret chargée est remplacée par `***` dans les messages, y compris
  dans les extraits de réponse, au cas où un serveur renvoie le jeton.
- Jamais de corps de requête dans les journaux. Les URL de suppression sont traitées comme sensibles : historique
  oui, journaux non.

## 5. File d'attente, reprise, lien

### 5.1 Persistance : SQLite ou fichiers JSON

- **Fichiers JSON par tâche** : simples, mais écritures concurrentes et états intermédiaires fragiles.
- **SQLite (`rusqlite`, mode WAL)** : transactions, requêtes pour l'historique.
- **Choix : SQLite**, idéalement la même base que l'historique, à décider avec l'équipe Après la capture.
- Table `upload_job` : id, fichier local (copie figée et déjà nettoyée de ses métadonnées, § 6), profil, état
  (`queued`, `running`, `waiting_unlock`, `waiting_network`, `failed_retryable`, `failed`, `done`, `cancelled`),
  tentatives, prochaine échéance, état de reprise (`UploadId` + ETag S3, offset SFTP, `partialToken` Zipline +
  horodatage), URL et URL de suppression obtenues, erreur expurgée.

### 5.2 Exécution et nouvelles tentatives

- La file tourne dans le processus résident. La CLI sans résident exécute l'envoi au premier plan avec la même
  logique et la même base, verrou par tâche. À arbitrer avec l'équipe Architecture.
- Au redémarrage, les tâches `running` repassent en `queued` et reprennent de leur état : S3 par les ETag,
  SFTP à l'offset, Zipline si le jeton a moins de 30 min (sinon depuis zéro), Imgur et HTTP depuis zéro.
- Nouvelles tentatives : backoff exponentiel avec gigue (5 s → 30 min), 8 tentatives par défaut, puis `failed`
  avec notification. Pas de nouvel essai sur les erreurs définitives.
- Hors-ligne : erreur de connexion ou DNS → `waiting_network`. On réessaie sur changement de réseau (NetworkManager
  sous Linux, `NWPathMonitor` sous macOS) ou au backoff, **sans sonde vers un hôte tiers**.
- Doublons : si la réponse se perd après un envoi réussi, un nouvel essai crée un doublon côté serveur. Les API
  visées n'ont pas de clé d'idempotence, on le documente.
- Concurrence : 2 envois simultanés, 1 par destination ; débit limitable.

### 5.3 Copie du lien et modèles

- Formats : `url`, `markdown` (`![{name}]({url})`, ou `[{name}]({url})` hors image), `html`
  (`<img src="{url}" alt="{name}">`, ~~ou `<video src controls>`~~ retiré avec la vidéo), ou modèle libre par profil :
  `link_template = "[{name}]({url}) ({width}×{height})"`.
- Variables : `{url}`, `{thumbnail_url}`, `{deletion_url}`, `{name}`, `{ext}`, `{width}`, `{height}`, `{size}`,
  `{date:%Y-%m-%d}`. Échappement automatique selon le format (HTML, Markdown).
- Copie après réussite seulement. Si plusieurs envois aboutissent, le dernier fini ne doit pas écraser
  silencieusement le presse-papiers ; cela se règle dans la chaîne d'actions.

## 6. Suppression des métadonnées avant envoi

- **Option A, réencodage complet** (décoder les pixels puis réencoder) : rien ne survit, mais c'est lent, avec perte
  en JPEG, et cela peut changer le profil colorimétrique.
- **Option B, nettoyage structurel sans réencodage** (`img-parts`) : on retire les segments ou chunks de
  métadonnées et on garde les pixels à l'identique.
- **Choix : B par défaut**, avec A en repli pour les formats non gérés.
  - PNG : on garde `IHDR PLTE IDAT IEND tRNS` et les chunks couleur (`sRGB gAMA cHRM iCCP`, plus `pHYs`). On retire
    `tEXt zTXt iTXt eXIf tIME` et les chunks inconnus.
  - JPEG : on retire APP1 (EXIF/XMP), APP13 (IPTC) et COM. ICC conservé, avec option pour le retirer.
  - WebP : on retire EXIF et XMP.
  - ~~GIF : extensions de commentaire et d'application XMP, via le flux de l'équipe Vidéo.~~ Retiré (décision du 11/11/2026).
- ~~**Vidéo (MP4)** : remultiplexage FFmpeg sans réencodage (`-map_metadata -1 -map_chapters -1 -fflags +bitexact
  -c copy`). On retire titre, `creation_time` et la chaîne `encoder`. À valider par test.~~ Retiré : plus de vidéo
  dans le produit, donc plus de FFmpeg.
- Le nettoyage s'applique à une **copie** placée dans la file. L'original local reste intact, sauf choix contraire.
- **Limites, à dire clairement** :
  - Le contenu visible (secrets à l'écran) n'est pas couvert ici : c'est le rôle de la détection assistée de
    l'équipe Annotation.
  - Le nom de fichier envoyé peut révéler des informations ; le modèle de nom d'envoi est distinct du nom local.
  - Le serveur ajoute ses propres métadonnées (date, IP, nom d'origine si `x-zipline-original-name`).
  - SFTP conserve un `mtime`.
  - Un format inconnu est envoyé tel quel **avec avertissement**, ou refusé si l'option « strict » est active.

## 7. Modèle de menaces

| Menace | Mitigation |
|---|---|
| Secret lu sur disque (sauvegarde, dépôt dotfiles) | Trousseau ou coffre chiffré ; config sans valeur ; coffre `0600` |
| `.sxcu` ou TOML malveillant qui redirige un jeton existant | Origines liées au secret (§ 4.5), confirmation au changement d'hôte, pas de redirection cross-origin |
| `.sxcu` partagé contenant le jeton de son auteur | Détection à l'import, rangement en trousseau, avertissement |
| Interception réseau | HTTPS obligatoire avec secret, vérification TLS, pas de contournement |
| Usurpation d'hôte SFTP | `known_hosts` strict, empreinte confirmée, refus si la clé change |
| Fuite par les journaux ou les rapports de bug | Expurgation par liste blanche et remplacement des valeurs connues |
| Fuite par les métadonnées (GPS, appareil, logiciel, date) | Nettoyage par défaut avant envoi (§ 6) |
| URL extraite piégée (`javascript:`, caractères de contrôle) copiée et collée | Validation `http(s)` stricte avant copie |
| Réponse énorme ou serveur lent (déni de service local) | Corps de réponse limité à 1 Mio, délais d'attente, annulation |
| Attaque hors ligne du coffre | ~~Argon2id 64 Mio, paramètres relevables~~ `age` (scrypt) avec phrase de passe robuste ; voir `docs/security/secrets.md` |
| Processus local du même utilisateur | Hors de portée (il lit déjà la mémoire et le trousseau déverrouillé) : à documenter |
| Requête non désirée (« local d'abord ») | Aucun profil par défaut, aucune sonde, aucune vérification de mise à jour dans ce module |

## 8. Points à transmettre aux autres équipes

- **Architecture** : un processus résident est nécessaire pour la file, la session du coffre et la surveillance du
  réseau. Comportement de la CLI sans résident à arbitrer. Formats de config : TOML proposé.
- **Licence** : `russh`, `russh-sftp` et `aws-*` sont en Apache-2.0, ce qui est incompatible avec une licence
  **GPL-2.0-only** ; c'est compatible avec GPL-3.0, MPL-2.0, MIT ou Apache. `kamadak-exif` est en BSD-2, `rexiv2`
  (GPL-3) est écarté.
- **Empaquetage** (Flatpak non distribué : pas de Flathub, décision du 11/11/2026 ; distribution .deb/.rpm/tap Homebrew/.dmg) : en Flatpak, l'accès au Secret Service passe par le portail (`org.freedesktop.portal.Secret`),
  dont la prise en charge par `keyring` reste à vérifier. Sur macOS en ad hoc, la confirmation Keychain revient à chaque
  mise à jour.
- **Après la capture / historique** : base SQLite partagée, URL de suppression stockée comme sensible, place de
  « Envoyer vers… » dans la chaîne d'actions et la miniature.
- ~~**Vidéo** : la commande FFmpeg de nettoyage est à intégrer au pipeline d'export ; les métadonnées GIF sont à
  traiter.~~ Sans objet : vidéo et GIF retirés (décision du 11/11/2026).
- **Annotation** : la détection de secrets à l'écran complète, sans remplacer, le nettoyage des métadonnées.
- **CLI** : sous-commandes `dest`, `queue`, `secrets`, options `--upload`, `--to`, `--copy`, `--json`.
- **i18n** : messages d'erreur d'envoi et de sécurité en FR et EN, sans détail technique sensible.

## 9. Questions ouvertes

1. ~~Imgur : intégration (A) ou simple modèle d'exemple (B, recommandé)~~ Tranché : B (modèle d'exemple).
2. ~~Licence du projet : confirmer qu'elle n'est pas GPL-2.0-only~~ Tranché : GPL-3.0-or-later.
3. Durée de session du coffre par défaut : 15 min d'inactivité ou jusqu'au verrouillage de l'écran ?
4. Lecture de `~/.ssh/config` (alias `Host`, `ProxyJump`) en v1 ou plus tard ?
5. Suppression distante depuis l'historique (Zipline, S3, SFTP, Imgur, `DeletionURL`) : v1 ou v2 ?
6. Partage d'une base SQLite unique entre historique et file ?
7. Options Zipline exposées dans l'interface (expiration, mot de passe, vues max) ou seulement en config ?

## Sources

- Zipline 4.8.3, code source :
  [dépôt](https://github.com/diced/zipline) (`src/server/routes/api/upload/partial.ts`,
  `src/lib/uploader/parseHeaders.ts`, `src/lib/api/partial.ts`, `src/lib/config/validate.ts`) ;
  [docs ShareX de Zipline (miroir)](https://www.mintlify.com/diced/zipline/integrations/sharex).
- ShareX : [doc custom uploader](https://getsharex.com/docs/custom-uploader) ;
  [`CustomUploaderItem.cs` et `Functions/`](https://github.com/ShareX/ShareX/tree/develop/ShareX.UploadersLib/CustomUploader) (tag v22.4.0).
- Imgur : [addclient](https://api.imgur.com/oauth2/addclient) (301 vers l'accueil, testé le 11/11/2026) ;
  [« The great API shutdown » (03/2026)](https://zonted.pages.dev/posts/the-great-api-shutdown/) ;
  [The Register, blocage au Royaume-Uni](https://www.theregister.com/2025/10/01/imgur_exits_uk/) ;
  [aide Flameshot sur Imgur](https://flameshot.org/docs/guide/imgur-help/) ;
  [apidocs.imgur.com](https://apidocs.imgur.com/) (non consultable par l'outil).
- S3 : [R2 multipart](https://developers.cloudflare.com/r2/objects/multipart-objects/) ;
  [compatibilité S3 de Garage](https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/) ;
  [API S3 de Backblaze B2](https://www.backblaze.com/docs/cloud-storage-s3-compatible-api) ;
  [intégrité des données dans les SDK AWS](https://docs.aws.amazon.com/sdkref/latest/guide/feature-dataintegrity.html) ;
  [incident R2 sur CRC32](https://www.cloudflarestatus.com/incidents/t5nrjmpxc1cj) ;
  [MinIO CE en 2026](https://glukhov.org/data-infrastructure/object-storage/minio-dead/).
- Crates (versions, dates, licences via l'API crates.io) :
  [reqwest](https://crates.io/crates/reqwest), [ureq](https://crates.io/crates/ureq),
  [object_store](https://crates.io/crates/object_store), [aws-sdk-s3](https://crates.io/crates/aws-sdk-s3),
  [rust-s3](https://crates.io/crates/rust-s3), [russh](https://crates.io/crates/russh),
  [russh-sftp](https://crates.io/crates/russh-sftp), [ssh2](https://crates.io/crates/ssh2),
  [keyring](https://crates.io/crates/keyring), [keyring-core](https://crates.io/crates/keyring-core),
  [secret-service](https://crates.io/crates/secret-service),
  [security-framework](https://crates.io/crates/security-framework), [age](https://crates.io/crates/age),
  [argon2](https://crates.io/crates/argon2), [chacha20poly1305](https://crates.io/crates/chacha20poly1305),
  [img-parts](https://crates.io/crates/img-parts), [little_exif](https://crates.io/crates/little_exif),
  [jsonpath-rust](https://crates.io/crates/jsonpath-rust), [rusqlite](https://crates.io/crates/rusqlite).
- [OWASP, stockage des mots de passe (Argon2id)](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html).
