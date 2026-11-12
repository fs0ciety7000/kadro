# Lot 2 — v0.2 Sélection, profils, S3, coffre : plan d'exécution

> Rédigé par `architect` le 12/11/2026, sans code. Sources : `docs/decisions.md` (prioritaire), `docs/plan.md` (lot 2),
> `cli.md`, `security/secrets.md`, `architecture.md`, `platform-linux.md`, `platform-macos.md`, `uploads.md`,
> `DESIGN.md`, `docs/memory/`, et le code à la fin du lot 1. **Validé le 12/11/2026 (recommandations retenues partout)** : les décisions du § 1 fixent 2.1, 2.3, 2.6,
> 2.7 et 2.8. Retours d'essai pris en compte : KDE Plasma 6.8.2, portail Screenshot **v2** (aucune cible), une boîte
> d'autorisation, data-control présent ; pas de GNOME chez l'utilisateur ; Mac plus tard, certificat 1.6 pas fait (le
> courtier Keychain refuse la signature ad hoc).

## 0. Le problème

- **Voulu** : `kropa capture area [--last] [--delay]` avec un overlay natif sur image figée (GTK, puis AppKit) ; les
  profils gérés par `kropa dest …` ; S3 ; le coffre `age` pour qui n'a pas de trousseau.
- **Contraintes** : `cli.md` validée (options, codes), `secrets.md` (IPC, coffre, S3), `DESIGN.md` § 8.1 ; la CLI ne
  dépend jamais de GTK ; local d'abord ; jamais de succès simulé ; portail v2 sans cibles, donc la zone se découpe
  dans l'image plein écran figée ; ni GNOME ni le Mac ne sont essayables par l'utilisateur pour l'instant.
- **Hors périmètre** : éditeur (touche `E` masquée jusqu'au lot 3), résident permanent (4.1), raccourcis (4.3),
  notifications (4.4), file et multipart S3 (4.5), SFTP (4.9), session du coffre (4.7), `capture output` découpé sous
  Linux (reste code 5 `output_not_isolable`), KWin ScreenShot2, screencopy wlroots.

## 1. Décisions qui reviennent à l'utilisateur (recommandation en premier)

1. **D1, lancement de `kropa-gtk`**. *Reco* : activation D-Bus (`DBusActivatable=true`, fichier
   `io.github.fs0ciety7000.Kropa.service`, `GApplication` en mode service avec délai d'inactivité) : environnement
   propre, **aucun descripteur hérité**, instance unique. *Sinon* : fork/exec par la CLI (plus simple ; hérite des fd,
   dont `--password-fd`, et de l'environnement).
2. **D2, descripteurs hérités** (`secrets.md` § 3.7, point « `unsafe` hors macOS » du relais). *Reco* : une
   exception `unsafe` bornée, un seul appel en tête de `main` de `kropa` et de `kropa-gtk` qui marque CLOEXEC tous les
   fd ≥ 3 (`close_range(CLOSE_RANGE_CLOEXEC)` sous Linux), dans un module revu par `security-reviewer` ; la règle 12
   d'`AGENTS.md` est amendée (inutile si `rustix` l'offre sans `unsafe`, à vérifier). *Sinon* : limite écrite
   (l'utilisateur lui-même est hors du modèle de menaces) ; `__clipboard-serve` garde le fd de la phrase tant qu'il vit.
3. **D3, emplacement de l'IPC**. *Reco* : nouvelle crate `kropa-ipc` (types, cadrage, client, serveur tokio) :
   `kropa-core` reste sans tokio. *Sinon* : module `kropa_core::ipc` derrière une fonctionnalité Cargo (comme le
   prévoit `architecture.md` § 6).
4. **D4, presse-papiers**. *Reco* : `kropa-gtk` copie l'**image** pendant que l'overlay a le focus (`area`, GNOME
   compris) et reste en vie tant qu'il possède la sélection ; le **lien** après envoi, ainsi que `upload`,
   `capture screen|output`, gardent `kropa __clipboard-serve` (KDE, wlroots) ; `cli.md` § 5 est amendé. *Sinon* :
   toute copie passe par `kropa-gtk`, lancé même pour `upload` (la CLI dépend alors d'une session graphique).
5. **D5, statut sous GNOME** (pas de data-control, donc aucune relecture possible). *Reco* : `copied` avec le champ
   ajouté `clipboard.verified: false` (aucun changement de version de schéma) ; `true` quand data-control a relu.
   *Sinon* : `unavailable` tant qu'on ne peut pas relire (une copie réelle serait annoncée ratée).
6. **D6, layer-shell**. *Reco* : fonctionnalité Cargo `layer-shell` de `kropa-gtk` (paquets Debian 13, Fedora,
   Ubuntu 26.04) et réglage `[overlay] surface = "auto" | "fullscreen"`, pour que l'essai KDE couvre aussi le chemin
   plein écran de GNOME. *Sinon* : plein écran partout en v0.2 (un seul chemin, avec les bogues d'empilement connus
   chez Spectacle).
7. **D7, Ubuntu 24.04** (décision 8 du plan) : `libgtk4-layer-shell` n'y existe pas (vérifié le 12/11/2026). *Reco* :
   on la garde ; overlay plein écran sur KDE, annoncé par `doctor`. *Sinon* : minimum relevé à Ubuntu 26.04 LTS.
8. **D8, GNOME**. *Reco* : GNOME marqué « non vérifié » (compat) ; critère de 2.3 = KDE à 100/200 % + sway/weston
   sans écran ; à rouvrir dès qu'un GNOME est disponible. *Sinon* : machine virtuelle GNOME (Fedora Workstation) chez
   l'utilisateur avant de clore le lot.
9. **D9, identifiant de clé S3**. *Reco* : `access_key_id` dans le TOML, option `--access-key-id`. Il n'est pas
   secret : il figure en clair dans chaque requête signée et dans les URL présignées, que le contrôle de sortie
   refuserait s'il était un secret. Seule la clé secrète va au magasin ; `secrets.md` § 1 est amendé. *Sinon* : deux
   entrées dans le magasin et une exception du détecteur pour l'URL présignée.
10. **D10, `dest add --type http`**. *Reco* : retiré de `add` (une API HTTP se décrit en TOML : `dest import`, modèle
    Imgur dans `docs/examples/`) ; `cli.md` § 2.3 est amendé. *Sinon* : squelette TOML commenté ouvert dans `$EDITOR`.
11. **D11, `secrets unlock|lock`** (question 4 de `cli.md`). *Reco* : reportés à 4.7 (code 5 d'ici là) ; 2.8 livre le
    coffre par processus, `--password-fd`, `use` et `migrate`. C'est un écart à la tâche 2.8 du plan validé.
    *Sinon* : livrés en 2.8, pour un `kropa-gtk` qui ne lit aucun secret au lot 2.
12. **D12, phrase proposée** (diceware de 6 mots ≈ 77 bits, `secrets.md` § 3.2). *Reco* : liste EFF de 7 776 mots,
    si `security-reviewer` confirme que sa licence (CC BY 3.0 US, des données) permet de l'embarquer, avec
    attribution. *Sinon* : aucune proposition en v0.2 (contrôle zxcvbn seul).
13. **D13, macOS : `kropa` → `kropa-app`** (à trancher au premier essai Mac). *Reco* : fils détaché (`posix_spawn` avec
    `POSIX_SPAWN_CLOEXEC_DEFAULT`, dans le module `spawn` de `kropa-keychain`, déjà autorisé à l'`unsafe`) ;
    attribution TCC à mesurer. *Sinon* : `.app` assistant imbriqué, lancé par LaunchServices (seconde permission TCC).

## 2. Désaccords et incohérences relevés (non tranchés ici)

1. `cli.md` § 5 : « `__clipboard-serve` n'existe plus dès que kropa-app (lot 2) possède la sélection », contre D4.
2. `cli.md` et `secrets.md` disent `kropa-app` pour les deux systèmes ; sous Linux, le binaire est `kropa-gtk`
   (`docs/memory/decisions.md`). Messages et docs doivent nommer le vrai binaire.
3. `cli.md` § 2.4 et plan 2.8 (`unlock` au 2.8), contre `secrets.md` § 3.6 (session du coffre au 4.7) : voir D11.
4. `cli.md` § 2.3 : `add --type http` irréalisable sans gabarit (D10). `--type sftp` : code 5 jusqu'au 4.9.
5. `secrets.md` § 1 range la clé d'accès S3 au magasin, contre la présignature (D9).
6. `uploads.md` § 4.2 place le coffre « dans le dossier de configuration » ; `secrets.md` § 3.3 dit
   `$XDG_DATA_HOME/kropa/` (`secrets.md` prime, `uploads.md` est à corriger).
7. `architecture.md` § 6 met les types IPC dans `kropa-core` (D3) ; `platform-linux.md` § 3.4 conseille D-Bus pour
   la CLI, mais la décision 5 du plan (socket) prévaut : D-Bus ne sert qu'à l'activation (D1).
8. La décision 8 du plan (Ubuntu 24.04) se heurte à l'absence de layer-shell (D7) ; la « check-list GNOME » de 2.3 est
   irréalisable (D8).
9. `DESIGN.md` § 8.1 prévoit la touche `E` (éditeur), qui n'existe qu'au lot 3 : masquée au lot 2 (principe 11 de
   `cli.md`).
10. Les chaînes de l'overlay manquent dans les `.ftl` (à ajouter en 2.3).
11. `secrets.md` § 4.6 : l'origine S3 est un « triplet », sans forme écrite, et `Placement` n'a aucune valeur pour
    SigV4 : définies en 2.7a, à reporter dans `secrets.md`.
12. Compose : Garage (profil `s3`) est servi en HTTP, alors que HTTPS est obligatoire avec un secret ; Caddy doit le
    couvrir (2.7a).

## 3. Dépendances nouvelles (vérifiées sur crates.io le 12/11/2026)

| Crate | Version (date) | Licence | rust-version | Usage, remarques |
|---|---|---|---|---|
| gtk4 | 0.11.5 (20/09/2026) | MIT | 1.92 | `kropa-gtk`, sous `cfg(target_os = "linux")` ; fonctionnalité `v4_14` (GTK 4.14.2 dans Ubuntu 24.04) |
| gtk4-layer-shell (+ -sys 0.6.1) | 0.8.1 (10/08/2026) | MIT | 1.85 | optionnelle (D6) ; bibliothèque C : Debian 13 1.0.4, Ubuntu 26.04 1.3.0, **absente d'Ubuntu 24.04** ; **doit être liée avant `libwayland-client`** |
| libadwaita | 0.9.2 (07/07/2026) | MIT | — | dépend de gtk4 ^0.11 ; seulement si le HUD en a besoin (sinon lot 3) |
| object_store | 0.14.2 (15/09/2026) | MIT/Apache-2.0 | 1.85 | S3 ; `default-features = false`, `aws-base` + `reqwest`, **sans `aws`** (qui active `reqwest/rustls`) ; faisabilité prouvée en tête de 2.7a |
| age | 0.12.1 (14/07/2026) | MIT OR Apache-2.0 | 1.74 | coffre ; `default-features = false` ; tire i18n-embed, rust-embed, des versions doublées de sha2, base64 et rand (avertissement de `deny`) |
| zxcvbn | 3.1.1 (24/04/2026) | MIT | 1.66 | force de la phrase de passe |
| toml_edit | 0.25.19 (06/11/2026) | MIT OR Apache-2.0 | 1.85 | réécrit `config.toml` et `destinations.toml` sans perdre les commentaires ; déjà dans le verrou (transitif) |
| png | 0.18.1 (14/02/2026) | MIT OR Apache-2.0 | 1.73 | découpe de la zone ; la même que tirera tiny-skia (lot 3) |
| tokio | 1.54.1 (déjà présente) | MIT | — | IPC : `net`, `rt`, `io-util`, `time`, `sync` ; `peer_cred()` Linux et macOS sans `unsafe` |

À vérifier au moment de l'ajout : `async-channel` (pont tokio → GTK), `dispatch2` (macOS, 2.4), la signature de
`rustix` `close_range` (D2). `zbus` 6.2.1 est déjà là (activation D1). MSRV 1.95 : compatible avec tout ce qui
précède. Chaque ajout : `cargo deny check` vert, et `security-reviewer` pour object_store, age et gtk4-layer-shell.

## 4. Ordre et parallélisme

| Vague | Tâches en parallèle (fichiers disjoints) | Attend |
|---|---|---|
| A | 2.1a, 2.2, 2.6, 2.7a, 2.8a, 2.9a | lots 1 et D2 |
| B | 2.1b, 2.1c, 2.7b, 2.8b | 2.1a ; 2.6 + 2.7a ; 2.6 + 2.8a |
| C | 2.3 ; 2.4 | 2.1b + 2.2 + 2.9a ; 2.1c + 2.2 |
| D | 2.5 ; 2.9b ; 2.10 | 2.3 (Linux), 2.4 (macOS) ; 2.5 |

- **Chemin critique** : 2.1a → 2.1b → 2.3 → 2.5 → 2.10.
- **Fichiers partagés, à fusionner l'un après l'autre** :
  - `crates/kropa-cli/src/{args,exec}.rs` : 2.6 → 2.7b → 2.8b → 2.5.
  - `kropa-core/src/config.rs` : l'écrivain arrive avec 2.6, puis `[secrets]` (2.8b) et `[overlay]` (2.3).
  - `*.ftl` : une section par tâche, ajouts seulement.
  - `.github/workflows/ci.yml` : seulement 2.1b (paquets GTK) et 2.3 (contrôle `ldd`).
  - `Cargo.toml` et `Cargo.lock` : rebase, puis `cargo build --workspace --locked` régénère ; jamais d'édition à la
    main.
- **Développeur seul** : la vague A peut se mener en série, dans cet ordre : 2.1a, 2.2, 2.6, 2.7a, 2.8a.

## 5. Tâches

### 2.1a IPC : protocole, client, lancement — `backend-dev` (+ `security-reviewer`)

- **Objectif** : socket Unix JSON-RPC 2.0, une ligne par message (≤ 64 Kio), `hello` versionné ; la CLI lance
  l'interface à la demande.
- **Fichiers** : `crates/kropa-ipc/` (nouvelle, D3), `crates/kropa-cli/src/{daemon.rs,doctor.rs,exec.rs}`,
  `Cargo.toml`, `AGENTS.md` (structure), `cli.md` (codes de raison).
- **Emplacements** : socket `$XDG_RUNTIME_DIR/kropa/app.sock` et `app.lock` (Linux) ;
  `~/Library/Caches/Kropa/app.sock` (macOS, < 104 octets, vérifié au démarrage).
- **Dossier** : créé `0700`, `lstat` (pas de lien), propriétaire = UID ; aucun repli vers `/tmp`.
- **Interfaces**
  - Constantes : `PROTOCOL: u32 = 1`, `MAX_LINE = 65_536`.
  - `runtime_dir() -> Result<PathBuf, IpcError>` (refus : `no_runtime_dir`, `unsafe_runtime_dir`).
  - `Client::connect(&Path)` : contrôle `peer_cred().uid == getuid()` **avant tout envoi**, puis `hello`.
  - `Client::call<P: Serialize, R: DeserializeOwned>(method, P)`.
  - `trait Launcher { fn launch(&self) -> Result<(), IpcError>; }`, avec `DbusActivation` (Linux, D1 :
    `StartServiceByName` de l'ID d'application) et `Spawn` (macOS, D13).
  - `ensure_running(&dyn Launcher, Duration) -> Result<Client, IpcError>` : interroge le socket pendant 5 s au plus.
  - Serveur : `serve(dir, Arc<dyn Handler>)` dans un fil tokio ; `flock` sur `app.lock` ; socket orphelin retiré sous
    verrou ; UID du client vérifié ; ligne trop longue = connexion fermée ; `secrets.*` jamais journalisé (aucune
    méthode `secrets.*` au lot 2).
  - `trait Handler: Send + Sync { fn call(&self, method: &str, params: Value) -> BoxFuture<Result<Value, RpcError>>; }`
  - `RpcError { exit_code: u8, code, reason: Option<&str>, message }`, transmis dans `error.data` de JSON-RPC.
- **Messages**
  - `hello` : `{protocol, client: "kropa", version}` → `{protocol, server: "kropa-gtk" | "kropa-app", version,
    methods: [...], features: ["layer-shell"?]}`. Protocole différent : `ipc_version_mismatch`.
  - `app.status` : `{version, protocol, owns_clipboard, overlay_open}`.
  - `app.quit`.
  - `capture.area` et `capture.cancel` : paramètres en 2.5.
- **Codes** : tout échec d'IPC = code 14. Raisons ajoutées à `cli.md` : `no_runtime_dir`, `daemon_start_failed`,
  `daemon_start_timeout`, `peer_uid_mismatch`, `ipc_version_mismatch`, `ipc_protocol_error` (`unsafe_runtime_dir`
  existe déjà).
- **Mises à jour** : `kropa version --json` rend `ipc_protocol: 1` ; `doctor` gagne une section IPC (chemin, état
  absent / à l'écoute / orphelin / non sûr, version du serveur, fichier `.service` présent).
- **Tests (conteneur)** : client et serveur réels dans un dossier temporaire ; UID injecté, plus `sudo -u nobody` dans
  les deux sens → refus, rien envoyé ; dossier `0755`, lien symbolique, autre propriétaire, sans `XDG_RUNTIME_DIR` →
  14 ; 64 Kio + 1 → fermé ; `hello` en protocole 2 → 14 ; activation sur un `dbus-daemon` privé (dossier de services
  temporaire, faux serveur de `kropa-ipc` en `test-support`) ; « zéro requête » toujours vert (`unshare -rn`).
- **Fin** : `cargo nextest run -p kropa-ipc -p kropa`, clippy et deny verts ; `security-reviewer` sans bloquant.

### 2.1b Serveur dans `kropa-gtk` — `linux-desktop-dev`

- **Objectif** : `GtkApplication` (ID `io.github.fs0ciety7000.Kropa`) en mode service, délai d'inactivité de 30 s,
  `hold()` tant qu'un overlay est ouvert ou que le processus possède la sélection ; serveur de 2.1a dans un fil tokio ;
  requêtes portées au fil GTK par un canal.
- **Durcissement** : `RLIMIT_CORE = 0` en release ; pas de `PR_SET_DUMPABLE` (portail) ; D2 appliqué.
- **Fichiers** : `crates/kropa-gtk/{Cargo.toml,src/*}` (gtk4 sous `cfg(linux)` : macOS ne compile pas GTK),
  `data/io.github.fs0ciety7000.Kropa.service`, `data/*.desktop` (`DBusActivatable=true`), `scripts/dev-install.sh`,
  `.github/workflows/ci.yml` (`libgtk-4-dev` dans chaque job Linux qui compile).
- **Tests (conteneur)** : `kropa-gtk` lancé sous `sway --headless` (`WLR_BACKENDS=headless`) et `dbus-run-session`,
  activé par `kropa doctor` → `hello` OK ; s'arrête seul après le délai ; deux activations = une seule instance.
- **Fin** : CI Linux verte avec les paquets GTK ; CI macOS verte (rien de GTK compilé). **Non vérifiable ici** :
  environnement d'activation d'une vraie session KDE (vérifié en 2.10).

### 2.1c Serveur dans `kropa-app` (macOS) — `macos-desktop-dev`

- **Objectif** : serveur de 2.1a dans un fil, renvoi au fil principal (runloop AppKit) ; lancement selon D13 ; durée de
  vie identique (inactivité).
- **Fichiers** : `crates/kropa-macos/src/*`, `crates/kropa-keychain/src/spawn.rs` (lancement détaché).
- **Fin** : CI `macos-15` verte : test client-serveur réel, `hello`, UID. **Non vérifiable ici** : rien ne se compile
  pour macOS depuis Linux (aws-lc-sys) ; essai réel au Mac.

### 2.2 Modèle de sélection commun et loupe — `backend-dev`

- **Objectif** : toute la logique de l'overlay sans écran (`DESIGN.md` § 8.1), commune à GTK et AppKit.
- **Fichiers** : `crates/kropa-capture/src/{selection.rs,region.rs,last_area.rs}` (portables), `png` ajoutée.
- **Interfaces**
  - `MonitorGeom { id: String /* connecteur ou n° */, logical: RectF, scale: f64 }`.
  - `map_monitors(&[MonitorGeom], image: (u32, u32)) -> Result<Vec<Monitor>, MapError>` : échelle uniforme seulement ;
    rapports incohérents → `MapError::LayoutMismatch` (code 10 `layout_mismatch`), jamais de devinette.
  - `Selection::new(monitors, active, last: Option<StoredArea>)` et `Selection::handle(Input) -> Outcome`, avec
    `Input` = `PointerEnter`, `Down`, `Move`, `Up`, `Key { key, shift, alt, primary }` et `Key` = flèches, `A`, `R`,
    `Tab`, `Enter`, `Escape`, `Help` (pas de `E`).
  - `Outcome` = `Redraw | Accepted(Region) | Cancelled | Hint(id) | Announce(id)`.
  - `Region { monitor, x, y, width, height }`, en pixels de l'image.
  - `hud_layout(&Selection, viewport) -> HudLayout` (dimensions, loupe, aide : priorité et retournement).
  - `magnifier(&RgbaView, center) -> Loupe` (15 × 15, hexadécimal, coordonnées).
  - `crop_png(&[u8], Region) -> Result<CleanPng, PngError>` : garde `sRGB`, `gAMA`, `cHRM`, `iCCP`.
  - `StoredArea { v: 1, image: (u32, u32), region }`, dans `$XDG_STATE_HOME/kropa/last-area.json` (macOS :
    `~/Library/Application Support/<ID>/`), fichier `0600`, `O_NOFOLLOW`.
- **Règles** : flèches = 1 px physique, Maj ×10, Alt redimensionne depuis la poignée ambre, Ctrl/⌘+A = écran actif,
  `R` = dernière zone (refusée si l'image a une autre taille), Entrée sans zone = indice, `Tab` absent avec un seul
  écran, zone limitée à un écran, petite zone sous 48 px logiques.
- **Tests (conteneur)** : unitaires et `proptest` (la zone reste dans l'écran ; aucun débordement aux bords) ;
  instantanés `insta` des positions du HUD ; loupe aux bords ; découpe octet pour octet sur fixtures 100 % et 200 % ;
  incohérence → erreur.
- **Fin** : `cargo nextest run -p kropa-capture` vert, sans écran.

### 2.3 Overlay GTK — `linux-desktop-dev` (+ `copywriter` pour les `.ftl`, `design-critic` sur captures)

- **Objectif** : à la demande `capture.area`, attendre `--delay`, appeler le portail (backend du lot 1, hors fil GTK,
  sa propre connexion zbus et `Registry`), afficher une surface par `GdkMonitor` avec l'image figée découpée par
  `map_monitors`, piloter `Selection`, puis rendre la zone.
- **Surfaces** : layer-shell (couche `overlay`, quatre ancres, zone exclusive -1, clavier `Exclusive` sur l'écran
  actif) quand le protocole est annoncé et que la fonctionnalité est compilée ; sinon `fullscreen_on_monitor` (GNOME,
  X11, Ubuntu 24.04, ou réglage `overlay.surface = "fullscreen"`, D6).
- **Dessin** : widget à `snapshot` (texture à l'échelle du moniteur, voile `k-veil`, poignées, loupe en
  `ScalingFilter::Nearest`).
- **Accessibilité** : annonces par `gtk_accessible_announce` (GTK 4.14).
- **Copie** : `--copy image` copie par `GdkClipboard::set_texture` **avant** de fermer, sous focus (D4/D5) ; sur
  data-control, relecture par une seconde connexion → `verified: true`.
- **Fichiers** : `crates/kropa-gtk/src/overlay/*`, `crates/kropa-core/src/config.rs` (`[overlay]`),
  `crates/kropa-i18n/i18n/{fr,en}.ftl` (aide, pastille « Écran figé · … », annonces).
- **Tests (conteneur, image `tests/gui` de 2.9a)**
  - Le chemin sans layer-shell compile et passe clippy ; le chemin avec layer-shell aussi (Debian 13).
  - `ldd kropa-gtk` : `libgtk4-layer-shell` avant `libwayland-client`.
  - Sway sans écran + faux portail (lot 1.4) : `wtype` Ctrl+A puis Entrée → zone = taille de l'écran ; deux sorties,
    `HEADLESS-2` à l'échelle 2, `Tab` ; Échap → code 3 ; `--copy image` → `wl-paste` rend les mêmes octets PNG.
  - Weston sans écran (plein écran) : l'overlay s'affiche, `capture.cancel` → code 130 côté CLI.
- **Fin** : tests verts dans le conteneur ; check-list KDE 100 % et 200 % ajoutée à `docs/testing/manual-linux.md`.
- **Non vérifiable ici** : GNOME (D8 ; risque de focus sans jeton d'activation), échelles mixtes réelles de KDE,
  coïncidence au pixel de l'image figée sur un vrai écran.

### 2.4 Overlay AppKit — `macos-desktop-dev`

- **Objectif** : un `NSPanel` sans bord par `NSScreen`, sous-classe qui peut devenir clé (`define_class!`), niveau
  écran de veille, `canJoinAllSpaces` | `fullScreenAuxiliary` | `stationary`, image SCK par écran (lot 1.5) capturée
  **avant** l'affichage (l'exclusion de la capture devient sans objet), dessin à `backingScaleFactor`, même
  `Selection`. Zone ramenée aux coordonnées de `capture screen` (`screens::layout`).
- **Fichiers** : `crates/kropa-macos/src/overlay/*` (`unsafe` permis, `// SAFETY:`).
- **Fin ici** : CI `macos-15` verte (compilation, tests du câblage par `NSEvent` synthétiques si le runner a une
  session graphique, sinon `#[ignore]`).
- **Au Mac** : Retina, multi-écran, Spaces, plein écran d'une autre app, TCC (D13) ; Dock pendant l'overlay à
  confirmer (la décision « Dock visible » vise les fenêtres durables).

### 2.5 `kropa capture area [--last] [--delay]` — `backend-dev`

- **Interface IPC**
  - `capture.area` : paramètres `{delay_ms, copy_image, lang, activation_token: Option}` (le jeton vient de
    `XDG_ACTIVATION_TOKEN` s'il est présent).
  - Résultat : `{file, region, output, scale, backend, captured_at, clipboard: {status, reason, verified} | null}`, le
    fichier étant écrit `0600` dans le dossier de `app.sock`.
  - Erreurs : codes du lot 1 relayés (3, 4, 5, 10).
  - `capture.cancel`, envoyé par la CLI sur SIGINT (code 130).
- **Comportement**
  - Sans `--last` : `ensure_running` → `capture.area` → la CLI relit le fichier (`O_NOFOLLOW`, propriétaire), passe
    `sanitize_png`, écrit le fichier final (`{mode}` = `area`), supprime le temporaire, enregistre `StoredArea`, puis
    envoie et copie comme au lot 1 (pas de seconde copie d'image si `kropa-gtk` l'a faite).
  - `--last` : **sans interface** ; capture d'écran du lot 1, puis `crop_png`. Aucune zone mémorisée → code 10
    `no_last_area` ; taille d'image changée → code 10 `last_area_stale`.
  - `--edit` : toujours code 5 (lot 3.7).
  - Un second `capture.area` pendant un overlay → code 10 `overlay_already_open`.
- **JSON** : `capture.mode = "area"`, `region`, `output`, `scale`.
- **Fichiers** : `crates/kropa-cli/src/{capture.rs,exec.rs,daemon.rs}`, `cli.md` (raisons), fixtures `trycmd`.
- **Tests (conteneur)** : `--last` avec faux portail, sans interface (découpe exacte, absent → 10, périmé → 10) ;
  `area` contre un faux serveur IPC (fichier rendu, annulation, SIGINT → 130, erreur 4 relayée) ; schéma JSON ; « zéro
  requête » ; puis bout en bout dans sway (2.9b).
- **Fin** : tests verts, `cli.md` à jour. **Au Mac** : `--last` et `area` macOS après 2.4.

### 2.6 `kropa dest add|edit|remove|set-default|export` — `backend-dev` (+ `security-reviewer`)

- **Syntaxe exacte** (`cli.md` § 2.3) :
  - `dest add <ID> --type zipline --url URL [--secret-stdin | --secret-fd N] [--default]` : jeton masqué sur le TTY ;
    sans TTY et sans source → 12 ; entrée `<ID>/token`, origine de `--url`, emplacement `header:Authorization`.
    `--type s3` arrive en 2.7b, `sftp` → 5 (4.9), `http` selon D10.
  - `dest edit <ID> [--url URL] [--set CLÉ=VALEUR]…` : clés de `--set` en liste blanche (format de lien, `kinds`,
    `prefix`, `public_base_url`, `presign`) ; jamais un gabarit qui porte un `{secret:}`. URL ou emplacement changé sur
    un profil à secret → confirmation **interactive seulement** (12 sans TTY, `--yes` sans effet). Sans option : copie
    temporaire `0600` ouverte dans `$EDITOR`, relue, validée (mêmes règles qu'un import), remplacement atomique,
    l'ancien fichier gardé si le résultat est invalide.
  - `dest remove <ID> [--yes]` : profil et entrées `<ID>/*` supprimés, défaut retiré s'il le visait.
  - `dest set-default <ID> | --none` : profil existant exigé (6).
  - `dest export [<ID>…]` : TOML sur stdout, `{secret:…}` seulement.
- **Code partagé**
  - `toml_edit` pour réécrire sans perdre les commentaires : `profiles::{replace_destination, remove_destination}`,
    `kropa_core::config::set_default_destination(dir, Option<&str>)`.
  - `SecretStore::names(profile) -> Vec<String>` (noms seulement), implémentée par `SecretServiceStore` et
    `KeychainStore`.
  - « `id` pris » = profil de la config **ou** entrée du magasin (N2).
- **Schémas** : ajout de `kropa.dest.change/1` (`action`, `id`, `default`, `secrets`) et de `kropa.dest.export/1` (avec
  `--json`) dans `cli.md` et `SCHEMAS`. Ajout de `docs/examples/imgur.toml` (modèle Imgur).
- **Fichiers** : `crates/kropa-cli/src/{dest.rs,args.rs,exec.rs}`, `crates/kropa-upload/src/{profiles.rs,secret.rs}`,
  `crates/kropa-store/src/{secret_service.rs,keychain_store.rs}`, `crates/kropa-core/src/config.rs`.
- **Tests (conteneur)** : `trycmd` de chaque branche (TTY simulé, `--no-input`, `--set` qui ressemble à un secret → 11,
  `--var` interdit) ; commentaires conservés ; export sans valeur (recherche de la valeur → absente) ; `$EDITOR`
  factice ; vrai gnome-keyring (CI) pour `remove` ; modèle Imgur importé contre `wiremock`.
- **Fin** : tests, clippy, deny verts ; `security-reviewer` sans bloquant.

### 2.7a Moteur S3 — `backend-dev` (+ `devops` pour le compose, `security-reviewer`)

- **Étape 1, preuve** : `object_store` compile avec `aws-base` + `reqwest`, sans `aws`, sur le client unique de
  `kropa-upload` (`with_http_connector`, mêmes TLS aws-lc-rs et `no_proxy`) ; sinon, retour à l'utilisateur avant
  d'aller plus loin (écart).
- **Interfaces**
  - `S3Settings { endpoint, region, bucket, path_style, prefix, public_base_url, presign, access_key_id }` (D9).
  - `trait S3Backend: Send + Sync { async fn put(&self, key, Bytes, content_type) -> Result<(), UploadError>; fn
    presign_get(&self, key, ttl) -> Result<Url, UploadError>; }`, implémenté par `ObjectStoreBackend` et un faux pour
    les tests.
  - `DestinationType::S3` ; `Placement::Sigv4` ; origine `s3:<https://hôte:port>|<région>|<bucket>` (triplet de
    `secrets.md` § 4.6).
- **Garde-fous** : identifiants statiques seulement, refus **avant** `build()` sans clé (jamais `from_env`, IMDS ni
  profils AWS) ; endpoint `http://` + secret → 11 ; pas de `x-amz-acl` ; `PutObject` simple (multipart au 4.5) ; lien
  = `public_base_url` + clé encodée, ou URL présignée ≤ 7 j ; erreur S3 → 9, corps en `MaskedText`.
- **TOML** : `type = "s3"`, `endpoint`, `region`, `bucket`, `path_style`, `prefix`, `public_base_url`, `presign`,
  `access_key_id` ; `[destination.secrets] secret_access_key = {}`.
- **Compose** : site Caddy `https://localhost:${KROPA_IT_S3_HTTPS_PORT}` → `garage:3900` ; `run.sh up --profile s3`
  initialise Garage (disposition, clé, bucket, droits) ; test `crates/kropa-cli/tests/s3_integration.rs`
  (`#[ignore]`, `run.sh test --profile s3`).
- **Fichiers** : `crates/kropa-upload/src/{s3.rs,destination.rs,secret.rs,engine.rs}`,
  `tests/integration/{Caddyfile,run.sh}`, `docs/testing/integration.md`.
- **Tests (conteneur, Docker)** : unitaires (style chemin et hôte virtuel, préfixe, encodage, 7 j au plus) ; `wiremock`
  (signature présente, aucun `x-amz-acl`, 403 masqué) ; `AWS_*` et `HTTPS_PROXY` définis → ignorés ; « zéro requête »
  sur `dest list` et `dest show` d'un profil S3 ; Garage réel (envoi + relecture par l'URL présignée). Hôte virtuel
  réel non testé (`*.localhost`) : unitaire seulement.
- **Fin** : `run.sh test --profile s3` vert ; `security-reviewer` sans bloquant.

### 2.7b S3 dans la CLI — `backend-dev`

- **Objectif** : `dest add <ID> --type s3 --bucket B --region R [--endpoint URL] [--path-style] [--prefix P]
  [--public-base-url URL | --presign DURÉE] [--access-key-id ID] [--secret-stdin | --secret-fd N] [--default]`
  (`--access-key-id` ajouté à `cli.md` si D9 est retenue) ; `upload`, `capture --upload=<s3>` et `dest test` vers S3.
- **Fin** : `trycmd`, puis envoi Garage de bout en bout par la CLI.

### 2.8a Coffre `age` (magasin) — `backend-dev` (+ `security-reviewer`)

- **Interfaces**
  - `VaultItems: ItemStore` : un seul fichier `age` (destinataire scrypt seul) qui contient le document v2
    (`kropa_keychain::store_doc`).
  - Lecture-modification-écriture sous `kropa_keychain::lockfile::acquire("store.lock")` : **le même verrou**, dans le
    même dossier que l'élément Keychain sous macOS.
  - Le magasin est `KeychainStore<VaultItems>` (nom `vault`) : sémantique, `names()` et raisons identiques au
    trousseau, sans code dupliqué.
  - `trait PassphraseSource { fn get(&self, Purpose) -> Result<Zeroizing<String>, Failure>; }`, avec
    `Purpose = Open | Create` : une phrase par commande, effacée à la fin.
- **Fichier** : `$XDG_DATA_HOME/kropa/secrets.age` (macOS : `~/Library/Application Support/<ID>/secrets.age`) ;
  `0600`/`0700` ; `O_NOFOLLOW`, `fstat` du descripteur et du dossier.
- **Écriture** : temporaire `O_EXCL`, `sync_all`, `rename`, `sync_all` du dossier ; temporaires de plus d'1 h retirés.
- **Facteur scrypt** : `log2 N` = 18 à l'écriture, 20 au plus à la lecture (réglage ≤ 22) ; moins de 16 →
  avertissement.
- **Raisons ajoutées** : `vault_locked`, `vault_wrong_passphrase` (message unique), `vault_unsafe_file`,
  `vault_work_factor` (toutes code 7) ; `weak_passphrase` (11) existe déjà.
- **Fichiers** : `crates/kropa-store/src/vault.rs`, `crates/kropa-store/Cargo.toml`, `crates/kropa-keychain/src/lib.rs`
  (documentation : ses modules portables servent aussi au coffre).
- **Tests (conteneur)** : menaces du § 7 de `secrets.md` (droits, lien, autre propriétaire avec `sudo`, panne avant
  `rename`, deux écrivains × 100 → 200 entrées, fixtures `log2 N` 21 et 30 refusées en moins d'1 s, stanza mêlée
  refusée) ; facteur abaissé par un constructeur de test ; un seul test à 18.
- **Fin** : verts ; `security-reviewer` sans bloquant.

### 2.8b Commandes du coffre — `backend-dev` (+ `security-reviewer`)

- **Commandes**
  - `secrets use keyring|vault` : écrit `[secrets] store` ; crée le coffre s'il manque (deux saisies sur le TTY, ou
    `--password-fd` ; refus < 12 caractères ou zxcvbn < 3 ; proposition selon D12) ; ne copie rien.
  - `secrets migrate --to vault|keyring [--yes]` : copie, relit, compare, puis supprime la source après confirmation ;
    s'arrête à la première erreur sans rien supprimer.
  - `--password-fd N` actif pour toute commande qui lit le coffre (4 Kio, refus de 1, de 2 et d'un fd à double usage).
  - `secrets status` donne l'état du coffre.
  - `unlock|lock` selon D11.
  - Aucune bascule implicite : magasin choisi indisponible → erreur.
- **Config** : `[secrets] store = "keyring" | "vault"` ; `[secrets.vault] work_factor = 18`, `max_work_factor = 20`.
- **Fichiers** : `crates/kropa-cli/src/{secrets.rs,secret_input.rs,args.rs,exec.rs}`, `crates/kropa-core/src/config.rs`,
  `crates/kropa-store/src/lib.rs` (`open_store(config)`), `.ftl`, `cli.md`.
- **Tests (conteneur)** : `trycmd` (phrase faible « kropa2026 » → 11, non interactif sans fd → 7, `store = keyring`
  sans Secret Service → erreur et coffre non créé) ; migration réelle depuis gnome-keyring (CI), arrêt sur erreur ; un
  fils `__clipboard-serve` lancé après `--password-fd` n'hérite pas du fd (D2, `/proc/<pid>/fd`).
- **Au Mac** : coffre sous macOS (compilé en CI). **Manuel** : déchiffrement de secours avec `age -d`.

### 2.9a Conteneur graphique et job hebdomadaire — `devops`

- **Contenu** : `tests/gui/Dockerfile` (debian:13 épinglée par digest : `libgtk-4-dev`, `libgtk4-layer-shell-dev`,
  `sway`, `weston`, `wtype`, `wlrctl`, `wl-clipboard`, `dbus`, `strace` ; paquets à vérifier à l'écriture),
  `tests/gui/run.sh` (local et CI), `.github/workflows/gui.yml` (chaque semaine + `workflow_dispatch`, `permissions:
  contents: read`, actions épinglées par SHA).
- **Fin** : `tests/gui/run.sh` vert dans ce conteneur (Docker) sur un test témoin ; `zizmor` vert.

### 2.9b Bout en bout chaque semaine — `qa-tester`, `devops`

- **Contenu** : les scénarios sway et weston de 2.3 et 2.5 dans `gui.yml`, avec et sans `layer-shell`.
- **Fin** : job vert deux semaines de suite ; échec = issue, jamais réessayé en silence.

### 2.10 [Utilisateur] Essais réels

- **KDE 6.8.2** : `docs/testing/manual-linux.md` § lot 2, avec `overlay.surface` `auto` puis `fullscreen` à 100 % et
  200 %, deux écrans si possible, `--copy image` puis collage, `--last`, S3 réel facultatif, coffre (`secrets use
  vault`, `migrate`).
- **GNOME** : D8. **Mac** : 2.4 et `--last` après 1.6.
- **Fin** : résultats notés dans `docs/decisions.md` et `compat.md`.

## 6. Risques

- **Liaison de layer-shell** : chargée après `libwayland-client`, elle ne prend pas → contrôle `ldd` en CI.
- **Focus sous GNOME** : une fenêtre plein écran lancée sans jeton d'activation peut ne pas recevoir le clavier, donc
  Échap et la copie ne marchent pas. À essayer sur GNOME ; on transmet `XDG_ACTIVATION_TOKEN` quand il existe.
- **Portail KDE v2** : boîte d'autorisation à chaque capture ? Composition de l'image en échelles mixtes inconnue →
  refus `layout_mismatch` plutôt qu'une zone fausse.
- **Activation D-Bus** : un environnement d'activation sans `WAYLAND_DISPLAY` → code 14 `daemon_start_failed`, dit par
  `doctor`.
- **object_store** en 0.x : trait `S3Backend`, test « zéro requête », preuve en tête de 2.7a.
- **age** alourdit l'arbre (i18n-embed, rust-embed, versions doublées) ; scrypt à 256 Mio coûte du temps et de la
  mémoire en CI → facteur réduit dans les tests.
- **Branches parallèles** : conflits sur `Cargo.lock` et `args.rs` → ordre de fusion du § 4.
- **Place disque** (~3 Go libres) : GTK + object_store ; vider `target/debug/incremental` avant les gros builds.
- **macOS** : attribution TCC de `kropa-app` (D13) ; courtier inutilisable avant 1.6 (sans effet au lot 2 : `kropa-app`
  ne lit aucun secret).

## 7. Décisions durables proposées (pour `docs/memory/decisions.md`, après validation)

- IPC dans `kropa-ipc` (D3) ; lancement de `kropa-gtk` par activation D-Bus (D1).
- Copie de l'image par `kropa-gtk` sous focus ; lien par `__clipboard-serve` (D4) ; champ `clipboard.verified` (D5).
- layer-shell en fonctionnalité Cargo, plein écran en repli et sur demande (D6, D7).
- `access_key_id` S3 non secret (D9) ; coffre = `KeychainStore<VaultItems>` sur `store_doc` et `store.lock`.
- La zone mémorisée vit dans les coordonnées de l'image de `capture screen` ; `--last` ne lance pas l'interface.
