# Modèle de sécurité des secrets — Kropa

> S.1 rédigée par `architect` le 11/11/2026, revue par `security-reviewer` (S.2, `secrets-review.md` : 2 bloquants,
> 10 importants, 12 mineurs), corrigée en S.3, contre-revue sans bloquant (N1 à N6), **décisions U1–U4 tranchées le
> 11/11/2026** (`decisions.md`) et N1, N3 précisés en S.4. Prime sur `uploads.md` § 4 et § 7 (Argon2id/XChaCha remplacé
> par `age`, vidéo retirée). Ajouts du § 9 reportés dans `cli.md`. « Doit » = exigence testable ; « non vérifié » = à
> tester.

## 0. Changements après revue (S.3)

### 0.1 Constats → corrections

| Constat | Correction (ou raison du refus) | § |
|---|---|---|
| **B1** sorties tirées de la réponse non contrôlées | `{responseurl}` sans query, fragment ni `user:pass@` ; `{header:}` interdit sur les en-têtes d'authentification ; **contrôle de sortie** unique sur toute valeur issue de la réponse (formes brute, percent, base64 aux 3 alignements, JSON, HTML) avant presse-papiers, SQLite, stdout, JSON, notification → code 11, rien copié ni enregistré. Écart assumé : `error` et l'extrait de réponse sont **masqués** (`***`) et non refusés, car l'envoi a déjà échoué (code 9) et le diagnostic reste utile. | 4.4 |
| **B2** import TOML, références inter-profils, emplacement libre | Références limitées au profil courant (`{secret:token}` = entrée `<profil>/token`, plus de `ref`) ; import `.toml` aux règles de `.sxcu` ; **emplacements** autorisés (`header:`, `query:`, `form:`, `body`) stockés avec les origines dans la valeur protégée, vérifiés à la résolution. | 2.3, 4.1–4.3, 5 |
| I1 phrase faible | Refus si `zxcvbn` < 3 ou < 12 caractères ; phrase diceware de 6 mots proposée. | 3.2 |
| I2 verrou perdu au `rename` | Verrou sur `store.lock` stable, relecture après verrou, jamais d'écriture depuis un cache. | 3.4 |
| I3 IPC non authentifiée côté client | Le client vérifie l'UID du serveur et le dossier ; aucun repli hors `XDG_RUNTIME_DIR` (code 14) ; ligne ≤ 64 Kio ; `secrets.*` jamais journalisé. | 1.2 |
| I4 bouclage HTTP squattable | `localhost` jamais ; IP littérales de bouclage seulement dans les builds de test. Option par profil : **décision U3**. | 4.5 |
| I5 trousseau Linux bloquant | Sans interaction possible : lecture de `Locked`, échec `keyring_locked` sans prompt ; appels D-Bus en `spawn_blocking` avec délai ; `Ambiguous` expliqué par `doctor`. | 2.1 |
| I6 deux binaires macOS | Un seul élément ; protocole 1.6 sur les deux binaires ; jamais `apple-tool:` ; courtier unique proposé (**U1**). | 2.2 |
| I7 expurgation incomplète | Même détecteur que B1 (formes dérivées) ; `-vv` n'ouvre que `kropa*` ; `HeaderValue::set_sensitive(true)`. | 6 |
| I8 `zeroize` surpromis | « Au mieux », copies connues listées, tampons pré-alloués, désérialisation empruntée ; `mlock` retiré ; swap chiffré conseillé. | 3.5 |
| I9 session du coffre mal bornée | Inactivité = aucune action humaine ; plafond 8 h ; `LockedHint`, `PrepareForSleep` (inhibiteur `delay`), fin de session ; test par compteur de `Drop`. | 3.6 |
| I10 secret dans le chemin (§ 4 contre § 5) | Interdit en v1 : import refusé avec message explicite. Autorisation sous B1 : **décision U2**. | 4.3, 5 |
| M1 IMDS d'`object_store` | Refus avant `build()` sans clés ; `with_http_connector` vers le client unique. | 4.6 |
| M2 proxy d'environnement | `.no_proxy()` (aucune variable lue, `cli.md` § 1.7) ; proxy explicite par config hors v1. | 4.5 |
| M3 CA privée | `rustls-platform-verifier` 0.7.1 (magasin système, où l'utilisateur ajoute sa CA) ; jamais « ignorer TLS ». `ca_file` par profil non retenu (un seul mécanisme). | 4.5 |
| M4 forme des origines | `url::Url::origin()` ; `user:pass@` refusé. | 4.1 |
| M5 réponse hostile | `regex` seul avec `size_limit`, XML sans DTD (`roxmltree`), `thumbnail_url` jamais téléchargée d'office, bidi et largeur nulle refusés. | 4.4 |
| M6 fichiers | `O_NOFOLLOW`, `fstat` du descripteur et du dossier, `*.tmp` orphelins retirés, SQLite `0600` ; `sync_all` suffit (F_FULLFSYNC sur Apple). | 3.4 |
| M7 descripteurs | 4 Kio max, `CLOEXEC`, refus de 1, 2 et d'un fd à double usage, un seul `\n` final retiré. | 3.7 |
| M8 `--var` d'authentification | Un `{inputbox:}` en emplacement de secret devient un secret à l'import ; `--var` vers un emplacement de secret → code 11. | 5 |
| M9 délai croissant | Retiré des mitigations (attaque hors ligne) ; déverrouillages sérialisés. | 3.6 |
| M10 `unsafe` | `prctl`, `setrlimit`, `flock` par `rustix` 1.1.6 ; pair par tokio ; `unsafe_code = "deny"`. | 6 |
| M11 incohérences | `Serialize` sur un type privé au magasin seulement ; magasin Apple = recherche puis modification (pas `SecItemUpdate`) ; `allow`, `use`, `migrate` proposés à `cli.md` (§ 9), `allow` interactif seulement. | 2.2, 6, 9 |
| M12 limites | § 7 : un processus du même utilisateur passe outre la confirmation ; proposition de vider le presse-papiers après collage ; X11 : frappe lisible, documenté. | 7 |
| Q1–Q8 | Intégrées : Q2, Q3, Q4, Q5, Q6, Q7, Q8 adoptées ; Q1 = **décision U1**. | 2–5 |
| Tests jugés irréalistes | Remplacés (UID injecté + `sudo -u nobody`, compteur de `Drop`, TLS `rcgen`, deux binaires × deux builds). | 8 |

### 0.2 Décisions tranchées (11/11/2026, `decisions.md`)

- **U1. Magasin macOS sans compte Developer : (b) courtier unique.** Seul `kropa` lit le Keychain et sert `kropa-app`
  par un canal anonyme hérité (§ 2.2) → **une invite par mise à jour**. Le coffre `age` reste disponible en option
  (`kropa secrets use vault`), proposé par `doctor` et au premier lancement. Le nombre d'invites reste à mesurer en 1.6.
- **U2. Secret dans le chemin de l'URL : (a) refusé en v1** ; emplacements limités à en-tête, query, champ et corps
  (§ 4.3, § 5).
- **U3. HTTP en bouclage avec secret : (a) builds de test seulement** ; aucune option par profil (§ 4.5).
- **U4. Ajouts à `cli.md` (§ 9) : acceptés**, reportés dans `cli.md` en S.4, sans nouveau code de sortie.

### 0.3 Contre-revue S.3 → précisions S.4

| Constat | Traitement | Où |
|---|---|---|
| **N1** masquage : ordre et étendue | Type `MaskedText` : détection sur le texte entier, masque étendu à la suite de l'alphabet, coupe ensuite | § 4.4, § 6 |
| **N3** courtier macOS | Méthode de vérification du pair, refus, zéro accès Keychain avant contrôle, réponse limitée au profil ; ce que (b) retire à (a) | § 2.2, § 7 |
| **N2** orphelins (1.7, 1.8) | Exigence d'implémentation : « `id` pris » = profil de la config **ou** entrée `<id>/*` du magasin ; `doctor` liste les entrées orphelines | § 5 |
| **N4** détecteur (1.7) | Exigence d'implémentation : normaliser percent (deux passes), HTML et JSON (deux passes, `+` compris), ignorer les blancs dans le base64, ajouter l'hex ; secret < 4 octets refusé à la saisie | § 4.4 |
| **N5** portée de l'expurgation (1.7, 1.8) | Exigence d'implémentation : « secret résolu » = **tout secret chargé dans le processus** depuis son démarrage, pas seulement ceux de la requête en cours | § 4.4, § 6 |
| **N6** entrée vide d'import (1.8) | Exigence d'implémentation : `secrets set` sur une entrée « manquante » créée par import remontre origines et emplacements et les fait confirmer | § 4.3, § 5 |

## 1. Inventaire et transit

| Secret | Destination | Forme | Saisi par |
|---|---|---|---|
| Jeton Zipline | HTTP (préréglage) | en-tête `Authorization` | TTY, stdin, dialogue, import |
| Clé d'accès + clé secrète S3 | `object_store` | identifiants statiques (jamais `from_env`, IMDS, profils AWS) | TTY, stdin, dialogue |
| Mot de passe SFTP ; phrase de passe de clé privée | `russh` | mot de passe ; la clé reste un fichier de l'utilisateur, jamais copiée | TTY, stdin, dialogue |
| En-têtes, arguments, paramètres d'une destination HTTP | `kropa-upload` | `{secret:nom}` dans un emplacement autorisé (§ 4.3) | TTY, stdin, dialogue, import `.sxcu`/`.toml` |
| Phrase de passe du coffre `age` | coffre local | jamais stockée, jamais gardée en mémoire après usage (§ 3.6) | TTY, `--password-fd`, dialogue |

Hors inventaire (sensibles mais non secrets) : URL de suppression (historique oui, journaux non), `SSH_AUTH_SOCK`
(chemin d'agent lu tel quel), clé privée de signature macOS (secret de CI, hors application).

### 1.1 Transit autorisé, et lui seul

Saisie (TTY masqué, stdin, descripteur, dialogue natif) → processus `kropa` ou `kropa-app` → magasin (§ 2, § 3). À
l'envoi : magasin → mémoire `Zeroizing` → requête TLS vers une origine **et** un emplacement autorisés (§ 4).
**Jamais** : argument de commande, variable d'environnement, fichier de config, base SQLite, presse-papiers, journal,
notification, stdout, sortie `--json`, URL (hôte, chemin) — y compris par écho du serveur (§ 4.4).

### 1.2 IPC entre CLI et résident (2.1, 4.7)

- Socket Unix JSON-RPC, ligne ≤ **64 Kio** (au-delà : connexion fermée). Linux : `$XDG_RUNTIME_DIR/kropa/` ; macOS :
  `~/Library/Caches/Kropa/` (chemin d'`architecture.md`, < 104 octets). Le dossier est créé `0700` par Kropa.
- **Les deux côtés vérifient** : le serveur, l'UID du client (`peer_cred()` de tokio = `SO_PEERCRED` / `getpeereid`) ;
  le client, **avant d'envoyer quoi que ce soit**, que le serveur a son UID (`peer_cred().uid == getuid()`), et que le
  dossier appartient à cet UID, est en `0700` et n'est pas un lien (`lstat`). Échec = code 14, rien envoyé.
- **Aucun repli** : sans `XDG_RUNTIME_DIR` (su, cron, SSH sans logind), pas de `/tmp` → code 14 « résident
  injoignable : pas de dossier d'exécution sûr ». La CLI reste utilisable sans résident (`cli.md` § 1.10).
- Méthodes qui portent une valeur : `secrets.set` et `secrets.unlock` seulement ; aucune ne **renvoie** de valeur (pas
  de `secrets.get`). Les paramètres de `secrets.*` ne sont jamais journalisés, même en `-vv`.
- Limite (§ 7) : un processus du même utilisateur peut appeler l'IPC comme la CLI.

## 2. Stockage principal : trousseau système

Crate **`keyring` 4.2.0** sur **`keyring-core` 1.0.1**, magasins choisis à la compilation :
`zbus-secret-service-keyring-store` 1.0.1 (Linux, fonction `rt-tokio-crypto-rust`, sans OpenSSL) et
`apple-native-keyring-store` 1.0.2 (macOS, module `keychain`) [1][2][3][4]. Autres magasins : **non retenus**.

**Un seul élément** sur les deux systèmes (Q5) : service `kropa`, compte `store`, valeur = le document du § 2.3, le
même que le clair du coffre. Un modèle, un verrou (§ 3.4), une invite macOS par mise à jour et par binaire. Coût
accepté : Seahorse/Trousseaux n'affichent qu'une entrée « Kropa — secrets », pas une par jeton.

### 2.1 Linux : Secret Service (gnome-keyring, oo7, ksecretd de Plasma 6, KeePassXC)

- **Attributs** : `service`, `username` ; libellé « Kropa — secrets » ; `target` **non utilisé** (collection séparée) [3].
- **Collection** : alias `default` (« login »).
- **Session de transport** : le magasin 1.0.1 impose `EncryptionType::Dh` [3] ; pas de repli `plain` ; un service
  sans DH = échec explicite (non vérifié, test en 1.8). Gain faible (bus du même utilisateur, DH 1024 bits), coût nul.
- **Verrouillé, sans interaction possible** (pas de TTY, `--no-input`, ou pas de session graphique pour le prompt,
  SSH compris) : Kropa lit la propriété `Locked` de la collection et de l'élément **avant** toute lecture et échoue
  `keyring_locked` (code 7) **sans prompt**. Le magasin 1.0.1 appelle `unlock_all` puis attend [3] : si son API ne
  permet pas de l'éviter, Kropa interroge `Locked` directement par zbus avant l'appel (à confirmer en 1.8).
- **Verrouillé, interaction possible** : prompt du service (`Unlock` + `Prompt`) ; refus ou fermeture =
  `keyring_locked`. Jamais de repli implicite vers le coffre.
- **Délais** : tout appel au magasin en `tokio::task::spawn_blocking` avec délai (5 s par appel, 120 s pour un prompt
  affiché) ; dépassement = `keyring_timeout` (code 7). Aucun script n'attend un humain.
- **Doublons** : `search_items` parcourt toutes les collections ; deux éléments `kropa`/`store` = `Ambiguous` → code 7
  `keyring_ambiguous`, jamais de choix arbitraire ; `doctor` dit combien et dans quelles collections (aucune valeur).
- **Absence de démon** (wlroots, X11 minimal, SSH) : pas de `org.freedesktop.secrets` ni d'activation →
  `keyring_unavailable`. Kropa ne lance pas de démon et propose le coffre (§ 3) par un message explicite.
- Flatpak : hors distribution v1 (`decisions.md`).

### 2.2 macOS : Keychain à fichier (login)

- Module `keychain` = trousseau « legacy » à fichier [4]. Module `protected` : réservé aux apps sandboxées avec
  entitlements de provisionnement [4][5], **non utilisable** sans compte Developer.
- **Contrôle d'accès, établi par la revue** sur la source Apple `securityd/src/clientid.cpp`
  (`partitionIdForProcess`) [11] :
  1. l'**ACL** désigne l'application par sa *Designated Requirement* ; avec le certificat auto-signé stable, la DR
     survit aux mises à jour [6] ;
  2. la **partition** : `teamid:` n'est donnée qu'au code Mac App Store, TestFlight, Developer ID ou Apple
     Development ; **tout autre code signé, auto-signé comme ad hoc, reçoit `cdhash:<hash>`**, qui change à chaque
     build.
- **Conséquence, dite honnêtement** : après **chaque mise à jour**, macOS affiche une invite « Kropa veut utiliser…
  » **par binaire** qui lit l'élément (`kropa` et `kropa-app` ont des cdhash distincts) ; « Toujours autoriser »
  ajoute le nouveau cdhash. Le certificat auto-signé garde son intérêt pour TCC (qui ne lit que la DR) et pour l'ACL,
  pas pour la partition. `platform-macos.md` § 4.4 (« pas d'invite ») est à corriger (§ 10).
- **Retenu (U1 = b) : courtier unique.** Seul `kropa` (la CLI, présente dans le paquet et dans `Kropa.app`) ouvre le
  trousseau → **1 invite par mise à jour**. `kropa-app` ne lit jamais le Keychain ; il lance `kropa __store-broker` pour
  chaque opération. Coût : `kropa-app` dépend du binaire `kropa` voisin. Repli au choix de l'utilisateur : coffre `age`
  (aucune invite Keychain, phrase de passe à chaque session du résident). Le chemin direct de `kropa` (CLI) est
  inchangé : il lit l'élément lui-même.

- Mitigations communes : un seul élément ; **modification en place** (le magasin Apple fait recherche puis
  modification [4], jamais suppression/recréation qui réinitialise l'ACL) ; notes de version qui annoncent l'invite.
  Jamais d'ajout de `unsigned:` ni de `apple-tool:` (partition de `security(1)` [11]), jamais de
  `security set-generic-password-partition-list` par Kropa.
- Invite refusée = `keychain_denied` (code 4), la tâche passe `waiting_unlock`, jamais de repli implicite.
- **Vérification prévue en 1.6** (Mac de l'utilisateur, macOS 15 et plus si possible 26) : build N signé, secret
  rangé et lu par `kropa` **et** `kropa-app` ; build N+1 signé du même certificat ; relever le nombre d'invites par
  binaire, l'effet de « Toujours autoriser », la survie de TCC ; refaire avec le courtier (§ 2.2.1, point 7).
  Résultat dans `decisions.md`.

#### 2.2.1 Courtier : vérification du demandeur (N3)

C'est le seul canal qui **rend** des valeurs à un autre processus (l'IPC du § 1.2 n'en rend aucune). Lancer
`kropa __store-broker` soi-même ne doit rien donner : sinon le courtier offrirait le document sans l'invite Keychain
que le chemin direct impose à un binaire inconnu.

1. **Canal** : `kropa-app` crée une paire `socketpair(AF_UNIX, SOCK_STREAM)` (`CLOEXEC`), lance le courtier par
   `posix_spawn` avec l'environnement vidé, une extrémité dupliquée en fd 3 seulement (fd 0, 1, 2 sur `/dev/null`), et
   garde l'autre. Pas de `pipe(2)` : un tube n'a ni `audit_token` ni pair identifiable. Pas de socket nommé.
2. **Contrôles, tous avant le moindre appel au Keychain**, dans cet ordre :
   - `ppid = getppid()` ; `ppid == 1` (parent mort ou lancé par launchd) → refus ;
   - fd 3 est un socket `AF_UNIX` connecté (`fstat` + `getsockopt(SO_TYPE)`), sinon refus (lancé depuis un shell) ;
   - `getsockopt(SOL_LOCAL, LOCAL_PEERTOKEN)` → `audit_token_t` du pair, et `LOCAL_PEERPID` ; PID du jeton = `ppid`,
     sinon refus (la paire vient d'un autre processus que le parent) ;
   - `SecCodeCopyGuestWithAttributes(nil, {kSecGuestAttributeAudit: jeton})`, puis `SecCodeCheckValidity` avec
     `kSecCSStrictValidate` contre l'**exigence** : identifiant de `kropa-app` **et** feuille de certificat = empreinte
     du certificat auto-signé du projet (la DR de `kropa-app`, compilée dans `kropa`). Le jeton porte la version de
     PID : un jeton périmé (processus réutilisé ou `exec` depuis la création de la paire) doit faire échouer l'appel ;
   - `SecCodeCopySelf` : le courtier lui-même est signé du même certificat, sinon refus (build non signé ou
     développement : courtier désactivé, `kropa-app` utilise le coffre ou signale `keychain_broker_unavailable`).
3. **Requête** : une ligne JSON ≤ 64 Kio, une seule par lancement, puis sortie. Opérations : `read <profil>` (entrées
   `<profil>/*` seulement, avec `origins` et `placements`), `write <profil>/<nom>` et `remove <profil>/<nom>` (sous
   `store.lock`, § 3.4), `status` (noms et compteurs, aucune valeur). **Jamais le document entier.** Avant de répondre,
   `getppid() == ppid` est revérifié (parent mort entre-temps → rien écrit). Délai total de 120 s (invite comprise).
4. **Refus** : `ppid` 1, fd 3 absent ou non socket, pair ≠ parent, signature invalide ou autre exigence, jeton périmé,
   courtier non signé, requête inconnue, malformée, multiple ou trop longue, profil absent. Refus = sortie non nulle,
   **zéro accès Keychain**, aucun détail sur stderr au-delà de la raison ; `kropa-app` le rend en code 7,
   `error.reason = "keychain_broker_refused"`.
5. **Ce qui tient la vérification** : `kropa-app` est signé avec le runtime renforcé, sans `get-task-allow` (§ 6) : un
   processus du même utilisateur ne peut ni s'y injecter (`DYLD_*`, `task_for_pid`) ni le déboguer. Sans cela,
   « le pair est `kropa-app` » ne prouverait rien.
6. **Mise en œuvre** : `SecCode` par `security-framework` (`os::macos::code_signing`, attribut de jeton d'audit) ;
   `LOCAL_PEERTOKEN` n'a pas d'enveloppe sûre connue : seule exception à `unsafe_code = "deny"` (§ 6), un bloc
   `#[allow(unsafe_code)]` local au module du courtier, revu par `security-reviewer`.
7. **Non vérifié, à prouver sur le Mac de l'utilisateur (1.6, puis test de 1.8)** : `LOCAL_PEERTOKEN` sur une paire
   `socketpair` rend bien le jeton du créateur ; un `exec` après création de la paire fait échouer
   `SecCodeCopyGuestWithAttributes` avec ce jeton (sinon, le cas « un processus crée la paire, lance le courtier puis
   fait `exec` de `kropa-app` » n'est plus couvert et relève du § 7) ; l'exigence avec feuille auto-signée est acceptée par `SecCodeCheckValidity` ; l'invite Keychain
   nomme `kropa` et ne revient qu'une fois par mise à jour ; `kropa __store-broker` lancé depuis Terminal (avec ou sans
   fd 3 forgé) → refus et **aucune** requête à `securityd` (`log stream --predicate 'subsystem == "com.apple.securityd"'`).

### 2.3 Document stocké (trousseau et coffre)

Un document JSON versionné, sérialisé dans un tampon `Zeroizing` :

```json
{"v":2, "entries":{"zipline/token":{
  "origins":["https://zipline.example.org:443"],
  "placements":["header:Authorization"],
  "created":"2026-11-11T14:00:00+01:00", "value":"…"}}}
```

- Clé d'entrée `<profil>/<nom>` ; `/` interdit dans un nom de profil ou de secret (code 6).
- **`origins`** (§ 4.1) et **`placements`** (§ 4.3) font partie de la valeur protégée : modifier le TOML en clair ne
  les change pas. Pour S3, l'origine est un triplet (§ 4.6).
- Version 1 (une entrée par élément, sans `placements`) : jamais livrée, aucune migration.

## 3. Repli : coffre `age` à phrase de passe

### 3.1 Format

- **Crate `age` 0.12.1**, recipient `scrypt` seul (stanza unique imposée par la spécification) [9][10]. Déchiffrable
  en secours par `age -d` ou `rage -d`. Fichier age v1 binaire ; clair = le document du § 2.3.
- **Facteur de travail** (Q3) : r = 8, p = 1 ; à l'écriture, `log2 N` **fixé à 18** (≈ 256 Mio), portable entre
  machines ; relevable par réglage. À la lecture, `set_max_work_factor(20)` ; au-delà, refus immédiat avec la valeur lue
  et le réglage `secrets.vault.max_work_factor` (≤ 22) pour cette lecture, l'écriture suivante revenant au réglage.
  Facteur lu < 16 → avertissement « coffre faible, à réécrire ». La marge de sécurité vient de la phrase (§ 3.2).

### 3.2 Phrase de passe (I1)

- À la création et au changement : refus si moins de **12 caractères** ou score **`zxcvbn` 3.1.1 < 3**, avec la raison
  (code 11, `weak_passphrase`). Kropa propose une phrase **diceware de 6 mots** (≈ 77 bits) tirée par `getrandom`.
- Phrase perdue = secrets perdus : dit à la création, avec la commande de ressaisie (`kropa secrets set`).

### 3.3 Emplacement et droits

- `$XDG_DATA_HOME/kropa/secrets.age` (Linux, défaut `~/.local/share`) ; `~/Library/Application Support/<App ID>/`
  (macOS). Hors dossier de config (dotfiles). Dossier `0700`, fichier **`0600`**.
- Ouverture en `O_NOFOLLOW` ; contrôles par **`fstat` du descripteur ouvert** et du dossier (pas de `stat` sur le
  chemin) : refus si autre propriétaire, lisible par groupe ou autres, ou dossier non `0700` (`doctor` explique).
- La base SQLite (URL de suppression) est créée `0600` dans un dossier `0700`.

### 3.4 Écriture atomique et concurrence (I2, M6)

- Verrou consultatif `flock` (via `rustix`) sur un fichier **stable** `store.lock` du même dossier, jamais sur
  `secrets.age` (le `rename` changerait l'inode). Le même verrou protège l'élément du trousseau (lecture-modification-
  écriture du document unique).
- Sous verrou : **relecture** du coffre, modification, temporaire `secrets.age.<aléa>.tmp` créé
  `O_CREAT|O_EXCL|O_NOFOLLOW` en `0600` dans le même dossier, écriture, `sync_all`, `rename`, `sync_all` du dossier.
  **Jamais d'écriture depuis un cache** : le résident relit toujours sous verrou.
- `*.tmp` orphelins de plus de 1 h retirés à l'ouverture, sous verrou. Chaque écriture tire un nouveau sel et une
  nouvelle clé de fichier (assuré par `age`).

### 3.5 Mémoire (I8)

- Phrase de passe, clair et valeurs dans `zeroize::Zeroizing` (`zeroize` 1.9.1) ou `secrecy::SecretString`, **au
  mieux** : des copies restent hors de notre contrôle — croissance de `Vec` dans `serde_json`, `Vec<u8>` de `keyring`,
  `String` de `rpassword`, `Bytes` de `HeaderValue`, tampons de rustls. Réduction : tampons pré-alloués à la taille
  maximale, désérialisation **empruntée** (`&str` dans le tampon `Zeroizing`) puis copie immédiate en `SecretString`.
- **Pas de `mlock`** (promesse non tenable). La doc conseille un swap chiffré et dit que vidage mémoire et
  hibernation peuvent contenir des secrets.

### 3.6 Déverrouillage (Q4, I9, M9)

1. *Par processus* (2.8) : sans résident, chaque `kropa` qui a besoin d'un secret demande la phrase (TTY masqué ou
   `--password-fd`), déchiffre, **efface la phrase aussitôt**, efface tout à la sortie.
2. *Par session du résident* (4.7) : `kropa secrets unlock` (ou dialogue) transmet la phrase par l'IPC ; le résident
   déchiffre, **efface la phrase**, et ne garde que les entrées déchiffrées. **Toute écriture** (`set`, `remove`,
   `allow`, `migrate`), rare, redemande la phrase. Repli noté si gênant, non retenu : identité X25519 chiffrée par
   phrase (format age standard), gardée à la place de la phrase.
- **Bornes** : verrouillage après **15 min sans action humaine** (réglable) — `unlock`, interaction avec l'interface,
  commande lancée depuis un TTY ; un envoi de la file ou d'un script sans TTY **ne prolonge pas**. Plafond absolu
  **8 h**. Verrouillage immédiat : Linux, `LockedHint = true` de la session logind (`Lock` n'est qu'une demande),
  `PrepareForSleep(true)` avec un inhibiteur `sleep` en mode `delay` pris à l'ouverture, fin de session ; macOS,
  `com.apple.screenIsLocked` et `NSWorkspaceWillSleepNotification`. `LockedHint` non vérifié par bureau : si
  l'événement n'est pas observable, `kropa secrets status` le dit. `kropa secrets lock` à la demande.
- **Mauvaise phrase** : message unique « phrase de passe incorrecte ou fichier altéré ». Pas de « délai croissant »
  (inutile contre l'attaque hors ligne) ; les déverrouillages sont **sérialisés** (un seul calcul scrypt de 256 Mio à
  la fois).
- **Non interactif** sans `--password-fd` ni résident déverrouillé : échec immédiat (code 7, « coffre verrouillé :
  `kropa secrets unlock` ou `--password-fd` »). Dans la file du résident : `waiting_unlock`, sans échec.

### 3.7 Descripteurs (`--password-fd`, `--secret-fd`) (M7)

Lecture jusqu'à EOF ou `\n` ; **4 Kio max** (au-delà : refus) ; un seul `\n` final retiré ; descripteur passé
`CLOEXEC` puis fermé. Refus de 1 et 2, et d'un descripteur à double usage (même fd pour phrase et secret, ou 0 quand
stdin porte un fichier). Pas de `--password`, pas de `KROPA_PASSWORD`.

### 3.8 Choix du magasin, bascule jamais silencieuse

- Trousseau s'il répond ; coffre seulement si l'utilisateur le **choisit** (`kropa secrets use vault`, ou réglage),
  y compris quand le trousseau est absent. `secrets.store = "keyring" | "vault"` ne change jamais tout seul.
- Magasin choisi indisponible, verrouillé ou refusé : erreur explicite, aucune lecture ni écriture dans l'autre.
- Migration explicite : `kropa secrets migrate --to vault|keyring` (copie, relecture, suppression de la source après
  confirmation).
- `kropa secrets status [--json]` : magasin et pourquoi, état (`unlocked`, `locked`, `unavailable`, `denied`,
  `ambiguous`), facteur scrypt, échéance du verrouillage, nombre d'entrées, observabilité du verrouillage d'écran ;
  **jamais** de valeur.

## 4. Liaison secret ↔ origine et emplacement, résolution

### 4.1 Origines

- Origine = `url::Url::origin()` (schéma, hôte IDNA en minuscules, IPv4 normalisée, point final retiré, port explicite
  ou par défaut du schéma). Une URL avec `user:pass@` est refusée (code 6). Fixée à la création, stockée **dans** la
  valeur protégée (§ 2.3).

### 4.2 Références (B2)

- **Une référence ne vise que le profil courant** : `{secret:token}` dans le profil `zipline` résout l'entrée
  `zipline/token`, et elle seule. La clé `ref` d'`uploads.md` § 2.2 est supprimée ; `[destination.secrets]` liste
  seulement les noms attendus (`token = {}`) pour que `dest show` signale ceux qui manquent. Aucune syntaxe ne désigne
  le secret d'un autre profil.
- Renommer l'`id` d'un profil ne déplace pas ses secrets (ils deviennent « manquants ») ; `dest remove` supprime les
  entrées du profil.

### 4.3 Emplacements autorisés (B2, I10)

- Valeurs : `header:<Nom>` (insensible à la casse), `query:<clé>`, `form:<champ>` (multipart ou urlencoded), `body`.
  Un `{secret:}` placé dans `{base64:…}` d'un en-tête relève de cet en-tête. `body` lie aussi l'**empreinte SHA-256 du
  gabarit de corps** : changer le gabarit vaut changer d'emplacement.
- Fixés à la création de l'entrée : préréglage (Zipline : `header:Authorization`), lieu où l'import a trouvé la
  valeur (§ 5), ou, pour `secrets set` d'une entrée neuve, lieux du gabarit du profil **montrés** et confirmés. Une
  entrée existante garde ses emplacements quand sa valeur est remplacée. Exigence (N6, 1.8) : `secrets set` sur une
  entrée vide créée par import remontre ses origines et emplacements et les fait confirmer, comme une entrée neuve.
- **Interdits partout** : hôte et chemin de l'URL (v1, **U2 tranchée**), nom de fichier, tout gabarit de réponse.
  Vérifié **au chargement** de la config (code 6).
- **À la résolution** (`kropa-upload`, juste avant l'envoi) : la requête est construite sans secret, son origine
  calculée ; chaque `{secret:x}` exige que l'origine figure dans `origins` de `x` **et** que son emplacement figure dans
  `placements` de `x` ; sinon code 11, **avant toute connexion**.
- `query:` est permis (certaines API l'exigent) mais signalé à la création : la query finit dans les journaux du
  serveur et des proxys.
- **Changement** d'URL ou d'emplacement d'un profil qui utilise un secret : l'autorisation ne suit pas.
  `kropa secrets allow <profil> <nom> --origin <URL> | --placement <P>` montre l'ancien et le nouveau et demande une
  confirmation **interactive seulement** (jamais `--yes`, code 12 sans TTY). Même règle pour `dest edit --url`.

### 4.4 Contrôle de sortie (B1, M5)

Tout ce qui est tiré de la réponse passe par **un seul point de contrôle** avant de quitter `kropa-upload`.

- **Gabarits de réponse** (`url`, `thumbnail_url`, `deletion_url`, `error`) : `{secret:}` interdit (chargement).
  `{responseurl}` rend l'URL finale **sans query, fragment ni `user:pass@`**. `{header:}` est refusé au chargement pour
  `authorization`, `proxy-authorization`, `cookie`, `set-cookie`, `www-authenticate` et tout en-tête d'un emplacement
  autorisé du profil.
- **Détecteur** (partagé avec l'expurgation, § 6) : pour chaque secret résolu `s`, et chaque valeur de requête rendue
  qui l'a contenu (`Bearer s`, sortie de `{base64:user:s}`), on cherche : forme brute ; percent-encodée (hex majuscule
  et minuscule, `+` pour l'espace) ; échappement JSON (`\/`, `\uXXXX`) ; échappement HTML ; base64 standard et URL-safe,
  avec ou sans `=`, aux **trois alignements** (partie stable de l'encodage). On cherche aussi la forme brute dans la
  sortie décodée une fois (percent, HTML). Un secret de moins de 8 octets est accepté avec un avertissement (faux
  positifs possibles : on refuse alors, côté sûr) ; moins de 4 octets : refusé (N4).
- **Sorties contrôlées** : URL, miniature, suppression, texte du lien (URL, Markdown, HTML), entrée d'historique
  SQLite, presse-papiers, notification, stdout, JSON. Les API d'historique, de presse-papiers et de sortie n'acceptent
  qu'un type `CheckedOutput` dont le seul constructeur applique le contrôle.
- **Détection** = code 11 (`secret_in_output`) : rien copié, rien enregistré dans l'historique au-delà d'une ligne
  d'échec sans URL, stdout reçoit le chemin du fichier local, JSON avec `upload.status = "failed"`, `url = null`,
  avertissement « le fichier a peut-être été envoyé ; lien retenu ». Échec définitif (pas de nouvel essai en file).
  Le message ne cite jamais la valeur.
- **`error` et extrait de réponse : type `MaskedText` (N1).** Ces textes sont masqués (`***`) et non refusés
  (l'envoi a déjà échoué, code 9), avertissement `secret_echoed`. Le seul constructeur,
  `MaskedText::from_response(corps, secrets, limite)`, applique dans cet ordre :
  1. décodage du corps **entier** (≤ 1 Mio, UTF-8 avec remplacement), jamais d'un extrait ;
  2. détection de toutes les formes ci-dessus sur ce texte entier ; chaque correspondance donne un intervalle ;
  3. **extension** de chaque intervalle à la **suite maximale** de caractères de l'alphabet de la forme trouvée, des
     deux côtés : base64 standard `A–Z a–z 0–9 + / =` et URL-safe `- _` (les 1 à 2 caractères de bord mêlent des bits
     du secret à ceux du voisin) ; percent : triplets `%XX` et caractères non réservés `A–Z a–z 0–9 - . _ ~`, `+` ;
     JSON : séquences `\uXXXX` et `\/` et caractères non réservés ; HTML : entités `&…;` et caractères non réservés ;
     brute : caractères non réservés et ceux du secret. Intervalles fusionnés s'ils se touchent ou se chevauchent ;
  4. remplacement de chaque intervalle par `***` (longueur fixe, aucune longueur révélée) ;
  5. **ensuite seulement**, coupe à 500 caractères sur une frontière de caractère, `…` ajouté.
  `MaskedText` n'a ni `From<String>` ni constructeur public autre ; stderr, journaux (`tracing`), message d'erreur
  Fluent, `error.message` et `upload.error` du JSON, colonne d'erreur de l'historique et notification n'acceptent que
  `MaskedText` (ou `CheckedOutput`) pour tout texte tiré de la réponse. *Test* : jeton (brut, percent, JSON, base64
  aux trois alignements) placé à chaque position de 495 à 530 d'un corps de 1 000 caractères → aucune sous-chaîne de
  4 caractères ou plus d'une forme du secret dans la sortie, et la suite base64 entière remplacée.
- **Exigences d'implémentation (contre-revue, 1.7)** : N4 — avant détection, normaliser percent (deux passes), HTML et
  JSON (deux passes, `+` d'un échappement partiel compris), ignorer les blancs et retours à la ligne dans le base64,
  chercher aussi l'hex (minuscules et majuscules) ; secret de moins de 4 octets refusé à la saisie (code 11). N5 —
  l'ensemble `secrets` du détecteur et de l'expurgation = **tout secret chargé dans le processus** depuis son
  démarrage, pas seulement ceux de la requête en cours.
- **Réponse hostile** : corps lu en flux et coupé à 1 Mio, délais de connexion et totaux, annulation ; `regex` 1.13
  seul (temps linéaire, `size_limit` et `dfa_size_limit` bornés), jamais `fancy-regex` ; XML par `roxmltree` 0.21
  sans DTD ; URL extraite `http(s)`, sans caractère de contrôle, de bidi (U+202A–202E, U+2066–2069) ni de largeur nulle ;
  `thumbnail_url` jamais téléchargée d'office.

### 4.5 Transport

- **HTTPS obligatoire** dès qu'un secret est résolu. `localhost` jamais (passe par le résolveur). `http://127.0.0.1`
  et `http://[::1]` acceptés **seulement dans les builds de test** (fonction Cargo non livrée) : un autre utilisateur
  peut écouter un port libre du bouclage (I4). **Aucune option par profil** (U3 tranchée).
- TLS par rustls avec **`rustls-platform-verifier` 0.7.1** (magasin système : une CA privée s'y installe) ; aucune
  option pour ignorer la vérification.
- Client `reqwest` unique, **`.no_proxy()`** (pas de lecture de `HTTP(S)_PROXY`) ; proxy explicite par config : hors
  v1.
- **Redirections** : `none` ou `same-origin` (défaut), 5 sauts au plus ; vers une autre origine = échec (même sans
  secret) ; HTTPS → HTTP interdit.
- En-têtes porteurs d'un secret : `HeaderValue::set_sensitive(true)` (masqué en `Debug`, hors table HPACK).

### 4.6 S3 et SFTP

- S3 (Q6) : l'« origine » est le **triplet** (endpoint canonique, région, bucket), styles virtual-host et path compris.
  SigV4 n'envoie pas la clé ; le risque est l'écriture dans un autre bucket : changer un élément du triplet demande la
  confirmation du § 4.3. Clés absentes = refus **avant** `build()` (jamais l'IMDS `InstanceCredentialProvider` [7]) ;
  `with_http_connector` branche le client unique (même TLS, même absence de proxy).
- SFTP : origine `sftp://hôte:port`, `known_hosts` strict (4.9). Un secret S3 ou SFTP ne sert qu'à son profil.

## 5. Import `.sxcu` et `.toml` (B2, Q7, M8)

Les deux formats suivent **les mêmes règles** ; un TOML reçu n'est pas plus fiable qu'un `.sxcu`.

- **Lu comme données** : ≤ 64 Kio ; `.sxcu` = schéma ShareX ≥ 13.7.1, `.toml` = tables `[[destination]]` seulement ;
  rien d'exécuté, aucune requête réseau, aucun `{inputbox:}`/`{select:}` résolu sans l'utilisateur.
- **Extraction** (Q7) : toute valeur d'en-tête, d'argument, de paramètre ou de corps est **présumée secrète**, sauf
  liste blanche (`Accept`, `Content-Type`, `User-Agent`, gabarits purs comme `{filename}`), montrée masquée (4 derniers
  caractères au plus) et **reclassable** par l'utilisateur ; `--yes` range le reste comme secrets. Chaque secret devient
  une entrée **neuve** du profil importé, liée à l'origine de l'URL et à l'emplacement où il a été trouvé.
- **`{secret:x}` dans un TOML importé** : crée une entrée **vide** (« manquante », `dest show`), à remplir par
  `kropa secrets set`. Un import ne réutilise **jamais** une entrée existante, d'aucun profil.
- **`id` déjà pris** : refus, sauf `--id <autre>` ou confirmation interactive « remplacer le profil X : ses secrets
  seront supprimés ». Pas d'écrasement sans confirmation, jamais d'héritage de secrets. Exigence (N2, 1.7 et 1.8) :
  « pris » = profil présent dans la config **ou** entrée `<id>/*` présente dans le magasin (orphelin d'un profil
  renommé ou retiré à la main) ; `doctor` liste les entrées orphelines, sans valeur.
- **Secret dans le chemin** de l'URL (segment à forte entropie : seul usage restant de l'entropie, Q7) : import refusé
  en v1 avec un message explicite (« jeton dans l'URL non pris en charge ») — **U2 tranchée**.
- **`{inputbox:}` d'authentification** (dans un emplacement de secret) : converti en secret à l'import, saisi masqué ;
  `--var` qui viserait un emplacement de secret → code 11 (secret en argument).
- **Confirmation** : résumé (origine, HTTPS ou non, emplacements, secrets détectés et masqués, query signalée) ;
  profil créé après accord ; en non interactif, `--yes` **et** `--expect-host <hôte>` exigés (hôte différent → code
  11), sinon code 12. Un import `http://` avec secret est refusé.
- Le fichier d'origine n'est pas recopié ; Kropa invite à le supprimer s'il contenait un secret en clair.

## 6. Journaux, erreurs, interfaces

- Types : `SecretValue` (et tout type qui en contient) implémente `Debug` et `Display` par `***`, sans `Serialize`
  public ; seul le type **privé** du document de magasin (§ 2.3) se sérialise, dans un tampon `Zeroizing`. `Clone`
  limité. Test qui échoue si un type secret dérive `Debug`.
- Couche `tracing` d'expurgation : en-têtes en **liste blanche** (`content-type`, `content-length`, `retry-after`,
  `location` sans query) ; le reste → `***`. Passe finale avec le **détecteur du § 4.4** (formes dérivées comprises)
  sur messages, erreurs et `--json` ; extraits de réponse et `error` n'y arrivent que sous forme de `MaskedText`
  (§ 4.4, N1). Jamais de corps de requête journalisé.
- `-vv` n'ouvre que `kropa*=debug` ; `hyper`, `h2`, `reqwest`, `rustls`, `russh`, `zbus`, `object_store` restent à
  `info`, sans réglage pour les ouvrir en release.
- Erreurs : `kropa-upload` renvoie des erreurs typées sans valeur ; erreurs de `reqwest` reformulées (URL avec query
  retirée) ; Fluent ne reçoit jamais de valeur.
- **Pas en argument** : pas de `--token` ; un argument ressemblant à un secret (`--header "Authorization: …"`) est
  refusé (code 11). **Pas en variable d'environnement** : aucune lecture de `KROPA_*`, `AWS_*`, `ZIPLINE_*`, ni des
  variables de proxy ; `object_store` sans `from_env`.
- `doctor` et rapport de bug : config avec noms de secrets seulement, aucun contenu de magasin.
- **Vidages mémoire** (Q8) : `prctl(PR_SET_DUMPABLE, 0)` et `RLIMIT_CORE = 0` (via `rustix`, `unsafe_code = "deny"`)
  dans `kropa-app` en release et dans `kropa` dès qu'il tient un secret : interdit aussi `ptrace` et
  `/proc/<pid>/mem` au même utilisateur non root [8]. Coût accepté : ni core dump ni gdb sans root (pas de rapport de
  plantage de toute façon, zéro télémétrie). Désactivé sous `debug_assertions`, jamais par variable. macOS : runtime
  renforcé (`codesign --options runtime`), sans `get-task-allow`.

## 7. Modèle de menaces

Hors périmètre, dit dans la doc : processus malveillant du **même utilisateur** (il lit la mémoire hors
`PR_SET_DUMPABLE`, le bus de session, le trousseau déverrouillé, et **appelle l'IPC** : les confirmations d'origine et
d'emplacement ne l'arrêtent pas), root, machine compromise, serveur de destination malveillant pour son propre jeton.
À documenter aussi : sous **X11**, la frappe dans un dialogue est lisible par tout client X ; le jeton **collé** dans
un dialogue reste dans le presse-papiers — le dialogue propose de le vider s'il contient la valeur saisie.

| Menace | Mitigation | Test qui le prouve |
|---|---|---|
| Secret lu sur disque (sauvegarde, dotfiles) | Trousseau ou coffre ; config avec noms seulement | grep des valeurs connues dans config, données, cache, journaux, SQLite, `*.tmp` → absentes |
| Coffre lisible par un autre | `0600`/`0700`, `O_NOFOLLOW`, `fstat` | coffre `0644`, lien symbolique, dossier `0755`, autre propriétaire (`sudo` en CI) → erreur dédiée |
| Coffre tronqué, écritures concurrentes | `store.lock` stable, relecture sous verrou, atomique | panne entre écriture et `rename` → ancien intact ; 2 écrivains × 100 → 200 entrées |
| Attaque hors ligne du coffre | Phrase robuste + scrypt 18 | phrase « kropa2026 » refusée ; facteur lu = réglage ; < 16 → avertissement |
| Fichier `age` piégé | `set_max_work_factor(20)` ; stanza unique | fixtures `log2 N` = 21 et 30 → refus < 1 s ; scrypt mêlé à un autre stanza → refus |
| Bascule silencieuse | `secrets.store` explicite | `dbus-run-session` sans Secret Service + `store = keyring` → erreur, coffre non créé |
| Trousseau qui bloque un script | `Locked` lu, pas de prompt, délais | service simulé verrouillé, sans TTY → code 7 en < 6 s, aucun `Prompt` appelé |
| TOML/`.sxcu` qui vise un jeton existant | Références au profil courant, import en entrées neuves | import visant `autre/token` ou `id` existant → refus ; `wiremock` : 0 requête |
| Secret déplacé (query, champ affiché) | Emplacements protégés | `wiremock` : `Authorization` déplacé en `query:key` ou `form:title` → code 11, 0 requête |
| Redirection vers un autre hôte | `same-origin`, 5 sauts | 302 vers autre port/hôte → échec, second serveur sans requête |
| Interception réseau | HTTPS + TLS vérifié | `http://` non loopback → refus ; serveur TLS avec certificat `rcgen` non approuvé → échec |
| Bouclage squatté | Pas d'exception en release | build de release : `http://127.0.0.1` + secret → code 11 |
| Secret dans le lien, l'historique, le presse-papiers | Contrôle de sortie § 4.4 | écho + `url = "{json:$.echo}"` → code 11, SQLite et presse-papiers factice vides, stdout = chemin ; écho base64 de Basic, percent, JSON → idem ; query secrète + `{responseurl}` → URL sans query |
| Serveur qui renvoie le jeton dans l'erreur | Masquage par le détecteur | réponse 400 avec le jeton (brut et base64) → stderr, journaux, `--json` avec `***` |
| Réponse énorme, lente, regex piège | 1 Mio, délais, `regex` borné | 2 Mio → erreur sans dépassement ; délai → `timeout` ; motif catastrophique → refus au chargement |
| Secret en argument ou en `--var` | Pas d'option, refus des motifs | `trycmd` : `--header "Authorization: …"` et `--var` vers emplacement secret → code 11 |
| Secret en variable d'environnement | Aucune lecture, `.no_proxy()` | `AWS_SECRET_ACCESS_KEY`, `HTTPS_PROXY` définis → ignorés, zéro requête au proxy |
| Fuite par `Debug`/panique | `Debug` masqué | `format!("{:?}")` d'un profil chargé ; panique provoquée pendant un envoi → stderr sans valeur |
| IPC usurpée (client ou serveur) | Contrôle d'UID des deux côtés, dossier `0700`, pas de `/tmp` | identifiants de pair injectés + CI `sudo -u nobody`, dans les deux sens → refus, rien envoyé |
| Coffre laissé ouvert | 15 min sans action humaine, 8 h, `LockedHint`, veille | horloge injectable ; envois périodiques sans TTY ne prolongent pas ; signaux logind simulés sur bus privé ; compteur de `Drop` des tampons |
| Invites Keychain à chaque mise à jour | Un élément, modification en place, courtier (U1) | manuel 1.6 : deux binaires × deux builds, invites relevées |
| Courtier appelé par un autre que `kropa-app` | § 2.2.1 : pair = parent, jeton d'audit, exigence de signature, réponse limitée au profil | `kropa __store-broker` depuis un shell, avec fd 3 forgé, ou parent signé autrement → refus, zéro requête à `securityd` |
| Secret coupé par l'extrait | `MaskedText` : détection sur le corps entier, masque étendu, coupe ensuite | jeton aux positions 495–530, toutes formes → aucun fragment ≥ 4 caractères |
| Usurpation d'hôte SFTP | `known_hosts` strict | clé changée → refus |
| `.sxcu` partagé avec le jeton de l'auteur | Extraction, masquage, entrées neuves | fixtures Zipline (en-tête), `Data` JSON, jeton dans le chemin (→ refus v1) |

## 8. Questions ouvertes

Toutes les questions de S.1 ont reçu une réponse (§ 0.1) et U1–U4 sont tranchées (§ 0.2). Reste le **non vérifié** :
invites Keychain réelles (15, 26), points du § 2.2.1 (7) sur le courtier, `LockedHint` par bureau, échec d'un service
sans DH, possibilité d'éviter `unlock_all` avec le magasin 1.0.1, audit RustSec des crates de stockage (à leur ajout).

## 9. Changements de `cli.md` (acceptés, U4 ; reportés en S.4)

1. **§ 2.4 `secrets`** : ajouter `kropa secrets allow <PROFIL> <NOM> (--origin <URL> | --placement <P>)`,
   interactif seulement (code 12 sans TTY, `--yes` sans effet) ; `kropa secrets use keyring|vault` ;
   `kropa secrets migrate --to vault|keyring` (confirmation, `--yes` sans TTY). Préciser que `set` d'une entrée neuve
   montre et fait confirmer origines et emplacements, et qu'avec le coffre toute écriture redemande la phrase, même
   déverrouillé.
2. **§ 2.3 `dest import`** : ajouter `--expect-host <HÔTE>`, exigé avec `--yes` sans TTY (code 12 sinon, 11 si l'hôte
   diffère) ; `.toml` soumis aux mêmes règles que `.sxcu` ; `id` existant refusé sauf `--id` ou confirmation
   interactive ; jeton dans le chemin refusé (selon U2). Ajouter `--expect-host` à la liste du principe 4.
3. **§ 2.3 `dest edit`** : la confirmation vaut aussi pour un changement d'emplacement d'un secret (pas seulement d'URL).
4. **§ 3 codes** : 11 couvre aussi `secret_in_output`, `placement_not_allowed`, `weak_passphrase`, `--var` vers un
   emplacement secret ; 7 couvre `keyring_locked` (sans prompt), `keyring_timeout`, `keyring_ambiguous` ; 14 couvre
   « dossier d'exécution absent ou non sûr ». Aucun code nouveau.
5. **§ 4 JSON** : ajouter `error.reason` (sous-cause machine : `origin_not_allowed`, `placement_not_allowed`,
   `secret_in_output`, `https_required`, `cross_origin_redirect`, `secret_in_argument`, `weak_passphrase`,
   `keyring_locked`…) — ajout de champ, sans changement de version ; avertissements `secret_echoed` et
   `uploaded_link_withheld`.
6. **§ 1.5 et options globales** : `--password-fd` et `--secret-fd` : 4 Kio max, refus des fd 1 et 2 et d'un fd à
   double usage.
7. **§ 2.1 `--var`** : refusé (code 11) quand la variable alimente un emplacement de secret.
8. **§ 2.4 `secrets set|remove <PROFIL> <NOM>`** : inchangé, cohérent avec la portée par profil (B2).

## 10. Suites hors de ce fichier (non modifiés en S.3 ni S.4, sauf `cli.md`)

- `uploads.md` § 2.2 : exemple `ref = "kropa/maison/token"` obsolète (→ `token = {}`) ; § 4.5 point 5 : exception
  `localhost` retirée (§ 4.5, U3) ; § 2.4 : `.toml` importé comme `.sxcu`.
- `platform-macos.md` § 4.4 : l'auto-signé n'évite pas l'invite Keychain (partition `cdhash:`).
- `architecture.md` (IPC) : contrôles du client et absence de repli du § 1.2 ; courtier macOS (U1 = b, § 2.2.1).

## Sources

[1] https://crates.io/crates/keyring (4.2.0) et https://docs.rs/keyring/4.2.0/keyring/ — magasins disponibles.
[2] https://crates.io/crates/keyring-core (1.0.1).
[3] https://docs.rs/zbus-secret-service-keyring-store/1.0.1/ et `src/service.rs` (`connect(EncryptionType::Dh)`,
    `unlock_all`) — attributs, collection par défaut, `target`, fonctions.
[4] https://docs.rs/apple-native-keyring-store/1.0.2/ et `src/keychain.rs` (recherche puis modification) ;
    `security-framework` 3.7.0 `src/os/macos/passwords.rs`.
[5] https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
[6] https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements
[7] `object_store` 0.14.2, `src/aws/builder.rs` (`InstanceCredentialProvider`, `with_http_connector`).
[8] man7.org : `unix(7)`, `user_namespaces(7)`, `PR_SET_DUMPABLE(2const)`, `ptrace(2)`.
[9] https://docs.rs/age/0.12.1/age/scrypt/ — calibration ≈ 1 s, `set_max_work_factor`, avertissement au-delà de 22.
[10] https://github.com/C2SP/C2SP/blob/main/age.md — stanza scrypt, r = 8, p = 1, stanza unique, STREAM 64 Kio.
[11] https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp — `partitionIdForProcess`
     (relevé par `security-reviewer`, S.2).
Versions relevées sur l'API crates.io le 11/11/2026 : `age` 0.12.1, `zeroize` 1.9.1, `secrecy` 0.10.3 (S.1) ;
`zxcvbn` 3.1.1, `rustix` 1.1.6, `rustls-platform-verifier` 0.7.1, `roxmltree` 0.21.2, `regex` 1.13.1 (S.3).
