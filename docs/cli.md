# Kropa : spécification de la ligne de commande v1

> État : **validée** le 11/11/2026 (`decisions.md`), avec les ajouts de sécurité de `security/secrets.md` § 9 (U4),
> reportés en S.4. Tâche 0.6 de `docs/plan.md`, rédigée par `architect` d'après `decisions.md`
> (prioritaire), `plan.md`, `product.md` (P4, P5), `uploads.md` et `architecture.md`. Les exemples sont en anglais,
> comme la CLI avec `LANG=en`. Contrat stable à partir de la v1.0 (SemVer, `quality-release.md` § 4.1).

## 1. Principes

1. **Verbe-nom** : `kropa <nom> <verbe>` pour les collections (`dest add`, `queue retry`), `kropa <verbe>` pour les
   actions directes (`capture`, `upload`, `edit`, `doctor`, `version`).
2. **Sortie humaine par défaut, `--json` stable** : avec `--json`, stdout contient **un seul objet JSON** (une ligne),
   y compris en cas d'erreur ; chaque objet porte `schema` (ex. `"kropa.capture/1"`). Un champ peut être ajouté sans
   changer de version ; retirer ou changer le sens d'un champ incrémente la version.
3. **stdout = résultat, stderr = messages** : stdout ne contient que le résultat utile (une URL, un chemin, une liste),
   sans couleur ni décoration ; progression, avertissements et erreurs vont sur stderr. `--quiet` coupe stderr sauf
   les erreurs.
4. **Pas d'interaction si stdin n'est pas un TTY** : aucune invite, aucune confirmation implicite. Ce qui exigerait
   une réponse échoue avec le code 12, sauf option explicite (`--yes`, `--expect-host`, `--secret-stdin`,
   `--secret-fd`, `--password-fd`, `--var`). Quelques confirmations de sécurité ne s'accordent **jamais** sans TTY,
   même avec `--yes` : changement d'origine ou d'emplacement d'un secret (`secrets allow`, `dest edit`).
   `--no-input` force ce comportement même sur un TTY. Les dialogues graphiques (overlay de sélection, portail,
   TCC) ne sont pas des interactions stdin : ils restent possibles.
5. **Aucun secret en argument ni en variable d'environnement** : jeton saisi masqué sur le TTY, lu sur stdin avec
   `--secret-stdin`, ou sur un descripteur avec `--secret-fd N` ; phrase de passe du coffre `age` par invite TTY ou
   `--password-fd N`. Une option du type `--token=…` n'existe pas ; une valeur ressemblant à un secret dans un argument
   (`--header 'Authorization: …'`) est refusée (code 11), comme un `--var` qui alimenterait un emplacement de secret.
6. **Langue** : `--lang fr|en`, sinon `LC_ALL`, `LC_MESSAGES`, `LANG` ; repli `en`. Seules les chaînes humaines
   changent : JSON, codes d'erreur et noms d'options sont invariants.
7. **Aucune variable d'environnement propre à Kropa** (règle du projet). Seules les conventions standard sont lues :
   `LANG`/`LC_*`, `NO_COLOR`, `XDG_*`, `HOME`, `EDITOR` (pour `dest edit`).
8. **Local d'abord** : aucune commande n'émet de requête réseau qu'un profil configuré ou une option explicite
   (`version --check`) n'a pas demandée. `doctor` n'en fait aucune.
9. **Jamais de succès simulé** : une étape impossible ou dégradée a un statut explicite (§ 5) et, si elle était
   demandée explicitement, un code de sortie dédié.
10. **Démon** : `kropa` est un client sans dépendance GTK. Les commandes à interface (`capture area`, `edit`) lancent
    `kropa-app` à la demande (IPC socket, lot 2.1) ; `upload`, `dest`, `secrets`, `history`, `doctor`, `version`
    fonctionnent sans lui, `queue` aussi (même base SQLite, verrou par tâche).
11. **Les options arrivent avec leur lot** : la CLI n'expose jamais une option non implémentée.

### Options globales

| Option | Effet |
|---|---|
| `--json` | Sortie machine (§ 4). |
| `--lang <fr\|en>` | Langue des messages. |
| `-q, --quiet` / `-v, --verbose` | Moins / plus de messages sur stderr (`-vv` : journal de débogage expurgé). |
| `--no-input` | Interdit toute invite, même sur un TTY. |
| `--color <auto\|always\|never>` | Couleur sur stderr ; `auto` respecte `NO_COLOR` et l'absence de TTY. |
| `--config <dossier>` | Dossier de configuration (tests, profils séparés). |
| `--password-fd <N>` | Phrase de passe du coffre `age` lue sur le descripteur N, pour cette commande seulement (lot 2.8). |
| `-h, --help`, `-V, --version` | Aide ; version courte (lot 0.3). |

**Descripteurs** (`--password-fd`, `--secret-fd`, et stdin pour `--secret-stdin`) : lecture jusqu'à EOF ou `\n`,
**4 Kio au plus** (au-delà : code 2, rien lu n'est gardé) ; un seul `\n` final retiré ; descripteur passé `CLOEXEC`
puis fermé. Refusés (code 2) : fd 1 et 2, un même fd pour la phrase et le secret, fd 0 quand stdin porte déjà un
fichier. Limites actives dès l'arrivée de chaque option (1.8 pour `secrets set`, 2.6 pour `dest add`, 2.8 pour
`--password-fd`).

## 2. Commandes v1

### 2.1 `capture` (lot 1 : `screen`, `output` ; lot 2.5 : `area` ; lot 3.7 : `--edit`)

```
kropa capture screen [OPTIONS]
kropa capture output [<NAME>] [OPTIONS]
kropa capture area   [--last] [OPTIONS]
```

- `screen` : tous les écrans assemblés (ce que rend le portail Screenshot). `output` : un seul écran, désigné par son
  nom de connecteur (`DP-1`, `eDP-1`) ou son nom macOS ; sans nom, l'écran principal. `kropa doctor` liste les noms.
- `area` : overlay de sélection sur l'image figée ; `Échap` annule (code 3).

| Option | Effet |
|---|---|
| `--delay <SEC>` | Attente avant la capture (0 à 60 s, décimales acceptées). |
| `--last` | `area` seulement : reprend la dernière zone sans overlay ; aucune zone mémorisée = code 10. |
| `--output <CHEMIN>` | Dossier de destination, ou fichier si le chemin se termine par une extension connue (`.png`). Défaut : dossier de la config. Écrasement refusé sans `--force`. |
| `--name <MODÈLE>` | Modèle de nom (`{date}`, `{time}`, `{host}`, `{mode}`, `{output}`, `{n}`, défini au lot 1.1) ; jamais de sortie hors du dossier. |
| `--upload[=<PROFIL>]` | Envoie après l'enregistrement local, vers le profil indiqué ou le défaut. Sans défaut configuré : code 6. |
| `--no-upload` | N'envoie pas, même si la config active l'envoi automatique. Exclusif avec `--upload`. |
| `--copy <url\|markdown\|html\|image\|none>` | Ce qui va au presse-papiers. Défaut : format de lien du profil si envoi, sinon `image` si la config le prévoit, sinon `none`. |
| `--edit` | Ouvre l'éditeur avant l'enregistrement final et l'envoi (lot 3.7). |
| `--var <NOM=VALEUR>` | Valeur d'une variable `{inputbox}` / `{select}` d'un profil importé de `.sxcu`. Refusé (code 11, `secret_in_argument`) si la variable alimente un emplacement de secret : un `{inputbox}` d'authentification devient un secret à l'import (lot 1.7). |

L'enregistrement local a toujours lieu avant l'envoi ; un échec d'envoi ne supprime jamais le fichier.

**Sortie humaine** : stdout reçoit une ligne, l'URL si l'envoi a réussi, sinon le chemin absolu du fichier.
stderr dit le reste (`Saved`, `Uploaded to zipline`, `Link copied (markdown)` ou `Link not copied: …`).

```sh
url=$(kropa capture screen --output ~/reports/ --name '{date}-{host}' --upload=s3-prod --copy none)
kropa capture area --last --delay 3 --upload
kropa capture output DP-1 --no-upload --json | jq -r .file.path
```

### 2.2 `upload` (lot 1.7)

`kropa upload <FICHIER> [--to <PROFIL>] [--copy url|markdown|html|none] [--var NOM=VALEUR] [--yes]`

Envoie un fichier existant vers `--to` ou le profil par défaut (`--var` : mêmes règles que `capture`). Les
métadonnées sont retirées de la copie envoyée
(lot 4.8), le fichier d'origine n'est jamais modifié. stdout : l'URL. `--yes` saute la confirmation d'un format
inconnu envoyé tel quel (`uploads.md` § 6). Exemple : `kropa upload ./diagram.png --to zipline --copy markdown`.

### 2.3 `dest` : destinations (lot 1.7 : `list`, `show`, `test`, `import` et `--expect-host` ; lot 2.6 : le reste)

```
kropa dest list | show <ID>
kropa dest add <ID> --type zipline|s3|sftp|http [--url URL] [OPTIONS_DU_TYPE] [--secret-stdin|--secret-fd N] [--default]
kropa dest edit <ID> [--url URL] [--set CLÉ=VALEUR]…     # sans option : ouvre le TOML du profil dans $EDITOR
kropa dest test <ID> [--yes]
kropa dest remove <ID> [--yes]
kropa dest set-default <ID> | --none
kropa dest import <FICHIER.sxcu|FICHIER.toml> [--id ID] [--yes --expect-host HÔTE]
kropa dest export [<ID>…]
```

- `list` / `show` : identifiant, type, origine, format de lien, défaut, magasin du secret ; jamais la valeur d'un
  secret. `show` signale les secrets manquants.
- `add` : options du type (S3 : `--bucket`, `--region`, `--endpoint`, `--path-style`, `--prefix`,
  `--public-base-url`, `--presign <DURÉE>` ≤ 7 j, lot 2.7 ; SFTP : `--host`, `--port`, `--user`, `--remote-dir`,
  `--public-url`, lot 4.9). Le secret est demandé masqué sur le TTY ; sans TTY, `--secret-stdin` ou `--secret-fd`
  sinon code 12. Il est stocké lié à l'origine de `--url`.
- `edit` : changer l'URL d'un profil qui a un secret, ou déplacer un `{secret:}` vers un autre emplacement (en-tête,
  query, champ, corps), exige une confirmation interactive, **jamais** accordée par `--yes` ni sans TTY (code 12) ;
  sans elle, l'autorisation ne suit pas (voir `secrets allow`).
- `test` : envoie une image témoin de 1×1 **après confirmation** (`--yes` sans TTY) ; affiche statut, en-têtes
  filtrés, URL extraite et lien formaté.
- `remove` : supprime le profil et ses secrets après confirmation (`--yes` sans TTY).
- `import` : `.sxcu` et `.toml` suivent **les mêmes règles** (`security/secrets.md` § 5). Résumé (origine, HTTPS ou
  non, emplacements, secrets détectés et masqués, query signalée) puis confirmation ; les secrets vont au magasin en
  entrées **neuves** du profil importé, le TOML ne garde que `{secret:…}` ; refus explicite des champs non pris en
  charge (`uploads.md` § 2.4). Sans TTY : `--yes` **et** `--expect-host <HÔTE>` exigés (code 12 sinon ; code 11 si
  l'hôte du fichier diffère). `id` déjà pris (config ou magasin) : refusé, sauf `--id` ou confirmation interactive de
  remplacement (les secrets du profil remplacé sont supprimés). Jeton dans le chemin de l'URL, ou import `http://`
  avec secret : refusé (code 11).
- `export` : TOML des profils sur stdout, références de secrets seulement, jamais les valeurs.

```sh
kropa dest add zipline --type zipline --url https://zipline.example.org    # token prompted, hidden
printf '%s' "$TOKEN" | kropa dest add team --type zipline --url https://z.team.example --secret-stdin
kropa dest import ./xbackbone.sxcu && kropa dest test xbackbone
kropa dest import ./team.toml --yes --expect-host z.team.example --no-input    # CI provisioning
```

### 2.4 `secrets` (lot 1.8 : `status`, `set`, `remove`, `allow` ; lot 2.8 : `unlock`, `lock`, `use`, `migrate`)

```
kropa secrets status | lock
kropa secrets set <PROFIL> <NOM> [--secret-stdin|--secret-fd N]
kropa secrets remove <PROFIL> <NOM> [--yes]
kropa secrets allow <PROFIL> <NOM> (--origin <URL> | --placement <P>)
kropa secrets use keyring|vault
kropa secrets migrate --to vault|keyring [--yes]
kropa secrets unlock [--password-fd N]
```

- `status` : magasin actif (trousseau ou coffre `age`) et pourquoi, état du coffre (verrouillé, ouvert, durée
  restante), secrets référencés mais absents. La bascule n'est jamais silencieuse.
- `set` : remplace la valeur ; une entrée existante garde ses origines et emplacements. Pour une entrée neuve, ou
  vide (créée par un import), les origines et emplacements tirés du profil sont **montrés et confirmés** (sans TTY :
  `--yes`). Avec le coffre, toute écriture (`set`, `remove`, `allow`, `migrate`) redemande la phrase de passe, même
  coffre déverrouillé. `remove` : supprime l'entrée.
- `allow` : autorise le secret pour une nouvelle origine (`--origin`, triplet endpoint/région/bucket pour S3) ou un
  nouvel emplacement (`--placement header:<Nom>|query:<clé>|form:<champ>|body`), après avoir montré l'ancien et le
  nouveau. **Interactif seulement** : sans TTY ou avec `--no-input`, code 12 ; `--yes` est sans effet. Hôte ou chemin
  de l'URL refusés comme emplacement (code 11).
- `use` : choisit le magasin (`secrets.store`), sans rien copier ; le magasin choisi n'est jamais changé
  automatiquement. `migrate` : copie toutes les entrées vers l'autre magasin, relit, puis supprime la source après
  confirmation (`--yes` sans TTY) ; s'arrête sans rien supprimer à la première erreur.
- `unlock` : ouvre le coffre `age` dans `kropa-app` pour la durée de session (lot 4.7) ; sans `kropa-app`, il est
  lancé. `lock` : verrouille immédiatement. Sur trousseau seul, les deux répondent « sans objet » (code 0, statut
  `not_applicable`).

```sh
kropa secrets unlock --password-fd 3 3< <(pass show kropa/vault)
kropa secrets allow zipline token --origin https://zipline.new.example.org    # interactive confirmation
kropa secrets use vault && kropa secrets migrate --to vault
```

### 2.5 `queue` (lot 4.5)

```
kropa queue list [--all] | retry <ID>|--failed | cancel <ID> | clear [--yes]
```

`list` : tâches actives (`queued`, `running`, `waiting_unlock`, `waiting_network`, `failed_retryable`) ; `--all`
ajoute `failed`, `done`, `cancelled`. `retry` relance une tâche (ou toutes les échouées) au premier plan si
`kropa-app` ne tourne pas. `clear` retire les tâches terminées (`done`, `failed`, `cancelled`), jamais une tâche
active. Les fichiers locaux ne sont jamais supprimés.

### 2.6 `history list` (lot 4.6)

`kropa history list [--search <TEXTE>] [--limit N] [--since <DATE>] [--json]`

Une ligne par capture ou envoi : date, chemin, profil, URL. `--search` porte sur nom, chemin, URL et profil. Les
URL de suppression n'apparaissent qu'en `--json` (sensibles, `uploads.md` § 4.6).
Exemple : `kropa history list --search grafana --json | jq -r '.items[0].url'`.

### 2.7 `edit` (lot 3.7)

`kropa edit <FICHIER> [--upload[=<PROFIL>]|--no-upload] [--copy …]`

Ouvre l'éditeur sur une image (PNG, JPEG) ; un `.kropa` à partir du lot 5.5 si la réouverture est livrée. La
commande attend la fermeture de l'éditeur ; stdout : chemin exporté (ou URL si envoyé). Fermeture sans export :
code 3.

### 2.8 `doctor` (lot 1.4 Linux, 1.5 macOS)

`kropa doctor [--json]` : système et bureau, capacités détectées (portails et versions, ScreenShot2, layer-shell,
data-control, GlobalShortcuts, TCC « Enregistrement de l'écran »), replis actifs, écrans et leurs noms, magasin de
secrets, socket et version de `kropa-app`, chemins de config, de données et de captures. Aucune requête réseau.
Code 0 même si des capacités manquent (c'est un rapport) ; code 1 seulement si le diagnostic lui-même échoue.

### 2.9 `version` (lot 1.3 ; `--check` au lot 5)

`kropa version [--check] [--json]` : version, commit, cible, version du protocole IPC et des schémas JSON.
`--check` interroge GitHub Releases **à la demande seulement** et dit si une version plus récente existe ; jamais de
téléchargement. Échec réseau : code 8.

## 3. Codes de sortie

| Code | Nom (`error.code`) | Cas |
|---|---|---|
| 0 | `ok` | Succès, y compris presse-papiers non copié quand la copie n'était pas demandée explicitement (§ 5). |
| 1 | `internal` | Erreur inattendue (bogue) ; message et, avec `-vv`, journal expurgé. |
| 2 | `usage` | Arguments invalides, options exclusives, sous-commande inconnue. |
| 3 | `cancelled` | Annulation par l'utilisateur : `Échap` dans l'overlay, « non » à une confirmation, éditeur fermé sans export. |
| 4 | `permission_denied` | Refus du portail Screenshot, permission TCC absente, accès au trousseau refusé par l'utilisateur. |
| 5 | `unsupported` | Fonction indisponible sur cette plateforme ou ce bureau (ex. `capture output <nom>` impossible à isoler). |
| 6 | `config` | Config ou destination invalide, profil ou id inconnu, aucun profil par défaut avec `--upload`. |
| 7 | `secret_unavailable` | Coffre verrouillé, phrase de passe fausse, secret référencé absent, trousseau inaccessible, verrouillé sans interaction possible (aucune invite affichée), délai dépassé, élément en double, courtier Keychain refusé (macOS). |
| 8 | `network` | DNS, connexion, délai, TLS : le fichier est conservé ; à partir du lot 4, la tâche est mise en file (`upload.job_id`). |
| 9 | `upload_failed` | Le serveur a répondu mais l'envoi a échoué : statut HTTP d'erreur, URL non extraite, réponse > 1 Mio. |
| 10 | `capture_failed` | Échec de capture hors permission (écran introuvable, pas de session graphique, `--last` sans zone). |
| 11 | `security_refused` | Origine ou emplacement non autorisé pour un secret, HTTPS exigé, redirection vers une autre origine, secret en argument ou dans un `--var`, secret renvoyé dans le lien ou la sortie, phrase de passe faible, hôte d'import différent de `--expect-host`. |
| 12 | `interaction_required` | Une réponse est nécessaire mais stdin n'est pas un TTY ou `--no-input` est actif. |
| 13 | — | Réservé (non utilisé : une copie impossible donne un avertissement et le code 0, voir § 5). |
| 14 | `daemon_unavailable` | `kropa-app` injoignable, version IPC incompatible (`hello`), dossier d'exécution absent ou non sûr (pas de repli vers `/tmp`). |
| 15 | `io` | Fichier introuvable ou illisible, écriture impossible, fichier existant sans `--force`. |
| 130 | — | Interruption (SIGINT). |

Quand plusieurs étapes échouent, le code est celui de la **première** étape en échec dans l'ordre capture →
enregistrement → envoi → copie ; les suivantes figurent dans le JSON. Les sous-causes ne créent pas de code : elles
vont dans `error.reason` (§ 4).

## 4. Schéma JSON de `capture` et `upload`

`schema` : `"kropa.capture/1"` ou `"kropa.upload/1"` (même structure, `capture` absent pour `upload`). Dates en
RFC 3339 avec fuseau ; tailles en octets ; coordonnées en pixels physiques.

```json
{
  "schema": "kropa.capture/1", "status": "ok", "exit_code": 0,
  "capture": { "mode": "area", "output": "DP-1", "scale": 2.0, "backend": "portal-screenshot",
               "region": { "x": 120, "y": 80, "width": 1280, "height": 720 },
               "captured_at": "2026-11-11T14:03:22+01:00" },
  "file": { "path": "/home/lea/Pictures/Kropa/2026-11-11_14-03-22.png",
            "format": "png", "width": 1280, "height": 720, "size": 482113 },
  "upload": { "status": "done", "destination": "zipline", "job_id": null, "error": null,
              "url": "https://zipline.example.org/u/a1b2c3.png", "thumbnail_url": null,
              "deletion_url": "https://zipline.example.org/api/user/files/a1b2c3" },
  "link": { "format": "markdown", "text": "![2026-11-11_14-03-22.png](https://zipline.example.org/u/a1b2c3.png)" },
  "clipboard": { "requested": "markdown", "explicit": false, "status": "unavailable",
                 "reason": "gnome_no_data_control" },
  "warnings": [ { "code": "clipboard_unavailable", "message": "Link not copied: GNOME does not allow it." } ],
  "error": null
}
```

| Champ | Valeurs |
|---|---|
| `status` | `ok` (tout ce qui était demandé a réussi), `partial` (résultat obtenu, étape secondaire en échec : code 8 avec file, 13), `error`. |
| `capture.mode` | `screen`, `output`, `area`. `backend` : `portal-screenshot`, `kde-screenshot2`, `wlr-screencopy`, `x11`, `screencapturekit`. |
| `upload.status` | `done`, `queued` (lot 4, `job_id` renseigné), `failed`, `skipped` (pas d'envoi demandé). `upload` vaut `null` avec `--no-upload`. |
| `clipboard.status` | `copied`, `pending` (notification « Lien prêt, Copier » en attente d'un clic), `unavailable`, `failed`, `skipped`. |
| `error` | `null` ou `{ "code": "security_refused", "reason": "origin_not_allowed", "message": "…", "step": "upload" }`, `code` = nom du § 3. |
| `error.reason` | Sous-cause machine, `null` si aucune ; liste ouverte (ajout sans changement de version) : `origin_not_allowed`, `placement_not_allowed`, `secret_in_output`, `https_required`, `cross_origin_redirect`, `secret_in_argument`, `weak_passphrase`, `import_host_mismatch`, `keyring_locked`, `keyring_timeout`, `keyring_ambiguous`, `keyring_unavailable`, `keychain_denied`, `keychain_broker_refused`, `unsafe_runtime_dir`. Premières valeurs au lot 1.7. Capture Linux (lot 1.4) : `no_session_bus`, `portal_unavailable`, `portal_declined`, `portal_dismissed`, `portal_not_allowed` (ces trois derniers avec le code 4), `portal_error`, `portal_no_response`, `portal_invalid_response`, `portal_file_unreadable`, `invalid_image`, `output_not_isolable`, `image_format_unsupported` (ces deux derniers avec le code 5). La clé `reason` est toujours présente (`null` sans sous-cause). |
| `warnings[].code` | Dont `secret_echoed` (le serveur a renvoyé le secret dans l'erreur : texte masqué) et `uploaded_link_withheld` (fichier peut-être envoyé, lien retenu car il contenait le secret). |

`deletion_url` est sensible : présente en JSON et dans l'historique, jamais dans stderr ni dans les journaux.
`error.message` et `upload.error` ne portent que du texte masqué (`***`) quand il vient de la réponse du serveur.
Les autres commandes ont leur propre schéma versionné (`kropa.dest.list/1`, `kropa.queue.list/1`,
`kropa.history.list/1`, `kropa.doctor/1`, `kropa.secrets.status/1`, `kropa.version/1`), décrits dans les fixtures
`trycmd` du lot 1.3 et dans la référence CLI du lot 5.6.

## 5. Presse-papiers selon la plateforme

| Plateforme | Image (`--copy image`) | Lien après envoi | Statut rendu |
|---|---|---|---|
| macOS (`NSPasteboard`) | copiée | copié | `copied` |
| KDE, wlroots (data-control) | copiée | copié ; le contenu est servi par `kropa-app`, ou par un processus fils détaché tant que le démon n'existe pas (lot 1) | `copied`, ou `failed` si le compositeur refuse |
| X11 | copiée | copié (même service que ci-dessus) | `copied` |
| GNOME Wayland (Mutter, sans data-control) | lot 1 : non copiée ; lot 2 : copiée pendant que l'overlay a le focus (`area`) | lot 1 à 3 : non copié ; lot 4 : notification « Lien prêt, Copier » | `unavailable`, puis `pending` ; jamais `copied` avant la copie réelle |

Règles : le statut reflète l'effet réel, vérifié quand l'API le permet. Sur GNOME, stderr dit
`Link not copied: GNOME does not allow clipboard access without focus. The link is printed above.` et le lien reste
sur stdout. Que la copie ait été demandée par la config ou explicitement par `--copy`, le code reste **0** avec cet
avertissement sur stderr et `clipboard.status` dans le JSON (décision de l'utilisateur du 11/11/2026). Un clic ultérieur sur « Copier » ne change pas
le code déjà rendu.

## 6. Arbitrages (validés le 11/11/2026)

1. **Noms fixés par le plan appliqués** : `capture screen|output|area` (pas `region`), `--upload[=profil]` et
   `--no-upload` (pas `--profile`), `--copy url|markdown|html|none` (pas `--link-format` ni `--no-clipboard`).
2. **`upload --to <profil>`** (comme `uploads.md`) plutôt que `--upload=profil`, redondant après le verbe `upload` ;
   `--profile` de `product.md` abandonné.
3. **`--copy image` ajouté** pour `capture` (copie de l'image sans envoi, parcours P2) ; hors liste du plan.
4. **stdout porte l'URL brute**, pas le lien formaté : le format choisi par `--copy` va au presse-papiers et dans
   `link.text` du JSON. Un script obtient ainsi toujours une URL utilisable.
5. **`queue clear` remplace `pause` / `resume`** de `uploads.md` (le plan cite `clear`, pas les deux autres).
6. **Découpage de `dest`** : `list`, `show`, `test`, `import` au lot 1 (nécessaires au critère Zipline du lot 1,
   avec `secrets set`) ; `add`, `edit`, `remove`, `set-default`, `export` au lot 2.6.
7. **`secrets set` et `secrets remove`** ajoutés au lot 1.8 (absents des documents d'équipe), pour ranger un jeton
   sans `dest add`.
8. **`--delay` dès le lot 1** pour `screen` et `output` (le plan ne le cite qu'au lot 2).
9. **`version --check` au lot 5** (avec la release, 5.3) : le plan ne le place dans aucun lot.
10. **Codes de sortie** : 15 codes nommés au lieu des cinq catégories de `product.md` ; refus de sécurité (11) et
    interaction requise (12) séparés de la configuration.
11. **`--var`** retenu pour les variables `{inputbox}` / `{select}` des `.sxcu` (`uploads.md` § 2.4).
12. **`--password-fd` global**, utilisable par toute commande qui lit un secret, en plus de `secrets unlock`.
13. **`capture output` sans nom** = écran principal (pas l'écran sous le pointeur, inconnu sous GNOME Wayland).
14. **Ajouts de sécurité** (S.4, `security/secrets.md` § 9, validés U4) : `secrets allow` (1.8, interactif seulement),
    `secrets use` et `secrets migrate` (2.8), `dest import --expect-host` (1.7), `error.reason` (1.7), limites des
    descripteurs (avec chaque option), refus de `--var` vers un secret (1.7). Aucun nouveau code de sortie.

## 7. Questions ouvertes

1. ~~Code 13 vs 0~~ — **tranché** : code 0 et avertissement (décision du 11/11/2026).
2. **`--output`** est à la fois dossier ou fichier selon l'extension : faut-il plutôt deux options
   (`--output-dir`, `--file`) pour éviter les ambiguïtés de chemins sans extension ?
3. **Isolation d'un écran sur GNOME** : le portail Screenshot ne rend que l'ensemble des écrans ; le découpage exige
   la géométrie des écrans (non vérifiée sans `DisplayConfig` de Mutter). Sinon, code 5 sous GNOME ?
   *Lot 1.4 : avec le portail (GNOME, KDE, wlroots), `capture output` rend le code 5 `output_not_isolable`, sans boîte de
   dialogue ni attente, et renvoie vers `capture screen`. À rouvrir avec la géométrie des écrans (lot 2.5).*
4. **`secrets unlock` avant le lot 4.7** : sans session du coffre dans le résident, `unlock` au lot 2.8 ne vaut que
   pour le `kropa-app` lancé ; faut-il le livrer seulement au lot 4.7 et se contenter de `--password-fd` au lot 2 ?
5. **Copie servie par un processus fils (lot 1, KDE/wlroots)** : acceptable avant le démon, ou copie reportée au
   lot 2 sur ces bureaux aussi ?
6. **Format d'image** : PNG seul en v1, ou `--format png|jpeg|webp` (non demandé par les documents) ?
7. **Échappement de `--name`** : liste définitive des variables du modèle de nom, à figer au lot 1.1.
