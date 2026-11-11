# Kropa : architecture et stack

> Proposition d'équipe du 11/11/2026. En cas de contradiction, docs/decisions.md prévaut.

> Équipe Architecture et stack. Proposition à valider, aucun code écrit.
> Versions et activité relevées le **11 novembre 2026** : API crates.io (`/api/v1/crates/<nom>`), dernier commit de la
> branche par défaut (clone superficiel des dépôts GitHub), recherches web citées en fin de document.
> Coût de maintenance noté pour **un développeur seul** : faible / moyen / élevé.

## 0. Synthèse

| Décision | Choix | Pourquoi en une ligne |
| --- | --- | --- |
| Interface | **Natif par plateforme** : GTK4 + libadwaita (gtk4-rs) sous Linux, AppKit (objc2) sous macOS, sur un cœur et un canevas communs | Seule option qui tient le glisser-déposer sortant, les fenêtres flottantes et l'accessibilité sur les deux systèmes |
| Capture | **Couche maison mince** sur des crates spécialisées : ashpd + pipewire (portails), libwayshot (wlroots), xcap (X11), screencapturekit (macOS) | Aucune crate unique ne couvre bien GNOME/KDE Wayland et macOS 15+ |
| Rendu | **tiny-skia** + cosmic-text, CPU, identique sur les deux plateformes | Pur Rust, mûr, sans GPU, rendu d'export identique au pixel près |
| Document | `.kropa` = ZIP (`document.json` versionné + `original.png`) | Rééditable, l'original non caviardé reste local |
| Vidéo | ~~GStreamer sous Linux, ScreenCaptureKit + AVFoundation sous macOS, GIF en Rust (`gif` + `imagequant`)~~ **Retirée (décision du 11/11/2026)** | Vidéo et GIF sont hors périmètre du produit |
| Licence | **GPL-3.0-or-later** (choix de l'utilisateur) | Compatible avec les dépendances Apache-2.0 ; ~~compatible avec FFmpeg GPL ou LGPL, x264, imagequant~~ (justification vidéo caduque) |
| Processus | Démon résident (l'application graphique) + CLI `kropa` cliente, IPC JSON sur socket Unix | Raccourcis, presse-papiers Wayland, file d'envoi et miniatures exigent un processus vivant |

## 1. Interface graphique

### Ce que Kropa exige d'un toolkit

Overlay de sélection plein écran sur image figée (multi-écrans, HiDPI) ; fenêtres sans bordure (miniature, épingle,
contrôles d'enregistrement) ; **glisser-déposer sortant d'un fichier** depuis la miniature ; zone d'icône (barre de
menus macOS, tray Linux) ; accessibilité (AT-SPI, NSAccessibility) ; i18n FR/EN ; binaire léger.

Contrainte qui pèse plus que le choix du toolkit : **sous GNOME Wayland, un client ne peut ni se placer, ni rester
au-dessus** (`gdk_toplevel` n'a pas de `keep_above` sous Wayland ; le protocole « always on top » est une demande de
fusion non adoptée, wayland-protocols MR 369), et Mutter n'implémente pas `wlr-layer-shell`. KDE et wlroots ont
layer-shell. Ces limites s'appliquent à **tous** les toolkits ; les équipes Plateformes doivent en tirer les replis.

### Options comparées

| Option | Avantages | Maintenance (seul) | Risque principal |
| --- | --- | --- | --- |
| **A. egui/eframe 0.37.0** (2026-10-15, MIT/Apache) | Un seul code, très actif, AccessKit intégré, prototypage rapide | Faible | Rendu non natif ; winit **n'a pas de glisser-déposer sortant** (PR winit #4571 encore ouverte en mai 2026) ; pas de layer-shell ; mode immédiat peu adapté aux longs formulaires |
| **A'. Slint 1.18.1** (2026-09-21, GPL-3.0 ou licences Slint) | Déclaratif, AccessKit, `@tr` gettext, GPL compatible avec Kropa | Moyen | Glisser-déposer limité à l'application (notes 1.17), dépend de la même PR winit ; style imité, pas natif |
| **A''. iced 0.14.0** (2025-12-07, MIT) | Architecture Elm propre, `iced_layershell` 0.19.1 pour KDE/wlroots | Moyen | **Pas d'accessibilité en amont** (fork `a11y-accesskit` seulement) ; pas de release depuis 11 mois malgré un dépôt actif ; mêmes limites winit |
| **A'''. Tauri 2.12.5** (2026-10-21) | Interface web, écosystème de plugins | Moyen | WebView (WebKitGTK sous Linux) lourd, contraire au principe « natif et léger » ; overlay et canevas lents |
| **B. GTK4 partout** (gtk4-rs 0.11.5, 2026-09-20) | Un seul code d'interface, natif sous Linux, a11y via AccessKit sous macOS depuis GTK 4.18 | Moyen | Sous macOS : aspect étranger, empaquetage des dylibs GTK à la main, pas d'`NSPanel`/agent de barre de menus, bogues de focus et de dépôt signalés en 2026 |
| **C. Natif par plateforme** : gtk4-rs + libadwaita 0.9.2 / objc2 0.6.5 + objc2-app-kit 0.3.2 | Rendu et comportement natifs ; `GtkDragSource` et `NSDraggingSource` ; `NSPanel` non activant, niveau flottant ; AT-SPI et NSAccessibility natifs ; binaires petits | **Élevé** (deux coquilles) | Double travail d'interface ; objc2 demande de l'`unsafe` et une bonne connaissance d'AppKit |

### Choix : C, avec une coquille aussi mince que possible

- Tout ce qui n'est pas widget vit dans des crates communes : modèle de document, outils et machine d'états de
  l'éditeur, rendu du canevas (le canevas est un tampon tiny-skia affiché dans une `GtkDrawingArea` ou une `NSView`),
  validation des profils d'envoi, requêtes d'historique. Les coquilles se limitent à lier des vues à ces modèles.
- La part dupliquée réelle : fenêtres, barres d'outils, préférences, historique, zone d'icône. Pour tenir le
  périmètre, la v1 garde des préférences simples ; les réglages avancés passent par le TOML et la CLI.
- **gtk4-rs directement, sans Relm4** (0.11.0, 2026-04-08, actif) : une couche de moins à suivre, et les deux
  coquilles gardent la même forme (présentateur commun, vues impératives).
- Pas de Qt : les liaisons Rust (cxx-qt) ajoutent C++ et CMake à la chaîne pour un gain limité face à GTK4, et KDE
  accepte bien une application GTK4.
- **Réversibilité** : si le double travail devient intenable, l'option de repli est B (GTK4 aussi sous macOS) ou A'
  (Slint) une fois le glisser-déposer sortant livré dans winit ; le cœur commun rend ce changement possible.

## 2. Capture

### Options comparées

| Option | Avantages | Maintenance | Risque principal |
| --- | --- | --- | --- |
| **xcap 0.9.10** seul (2026-11-09, Apache-2.0) | API unique, très actif | Faible | Wayland marqué ⛔ « pas entièrement pris en charge » ; s'appuie sur `org.gnome.Shell.Screenshot` (réservé aux appelants autorisés) puis le portail ; macOS via `CGWindowListCreateImage`, **obsolète depuis macOS 15** |
| **scap 0.0.8** (2024-12-10 ; 0.1.0-beta.1) | Pensé pour l'enregistrement, PipeWire sous Linux | Faible | **Dépôt inactif** depuis 2025-08-05, aucune release stable depuis 2 ans |
| **Couche maison sur crates spécialisées** | Chaque cas utilise la bonne API ; on maîtrise les replis | Moyen | Plus de code d'intégration ; tests manuels sur chaque bureau |

### Choix : trait `Capturer` maison, un back-end par environnement

| Environnement | Capture d'image | Flux (vidéo, fenêtre, défilement) | Crates |
| --- | --- | --- | --- |
| GNOME, KDE (Wayland) | portail `Screenshot` (non interactif après première autorisation), puis overlay maison sur l'image figée | portail `ScreenCast` + PipeWire (choix fenêtre/écran dans la boîte du portail, jeton de restauration) | ashpd 0.14.0, pipewire 0.10.1 |
| wlroots (Sway, Hyprland) | `ext-image-copy-capture` / `wlr-screencopy` | idem ou portail wlr | libwayshot 0.10.0 |
| X11 | XGetImage / XComposite | x11rb ou xcap | xcap 0.9.10 (X11 seulement) |
| macOS | `SCScreenshotManager` (macOS 14+) | `SCStream` (audio système 13+, micro 15+) | screencapturekit 11.1.2 |

- Le modèle « image figée puis overlay » est le seul qui marche partout sous Wayland : on capture l'écran entier,
  on affiche une fenêtre plein écran par moniteur avec cette image, la loupe, les dimensions et la pipette. Sur KDE
  et wlroots, `gtk4-layer-shell` 0.8.1 donne un overlay plus propre (calque `overlay`, clavier exclusif).
- **macOS** : on préfère screencapturekit-rs (très actif, 11.x) aux liaisons brutes `objc2-screen-capture-kit`, qui
  restent disponibles pour un manque ponctuel. Version minimale proposée : ~~macOS 14~~ **macOS 15 Sequoia** (décision du 11/11/2026).
- La fenêtre « propre » (ombre, fond) se fait par composition dans `kropa-render` à partir d'une capture de fenêtre ;
  sous GNOME Wayland, la sélection de fenêtre passe forcément par la boîte du portail.

## 3. Rendu des annotations et format de document

### Moteur de rendu

| Option | Avantages | Maintenance | Risque principal |
| --- | --- | --- | --- |
| **tiny-skia 0.12.0** (2026-02-02, BSD-3) | Pur Rust, CPU, mûr (base de resvg), anticrénelage de qualité, aucune dépendance système | Faible | Pas de texte ni de flou : à ajouter (cosmic-text, flou maison) ; CPU suffisant mais à mesurer en 5K |
| **vello 0.11.0** / vello_cpu 0.3.0 (2026-10-02) | Moderne, GPU (wgpu), très actif | Moyen | API encore mouvante (0.x à chaque trimestre), wgpu lourd ; vello_cpu jeune |
| **skia-safe 0.157.0** | Skia complet (texte, filtres, flou) | Moyen | Binaire Skia précompilé ou compilation C++ longue ; +20 Mo |
| **femtovg 0.28.0** | Léger, OpenGL | Moyen | Dépend d'un contexte GL dans chaque coquille |
| **Cairo (cairo-rs 0.22.9)** | Natif sous GTK | Faible sous Linux | Dépendance système lourde sous macOS ; rendu différent selon la plateforme |

**Choix : tiny-skia** pour l'affichage du canevas et l'export, avec **cosmic-text 0.19.0** (mise en forme, swash pour
la rastérisation, polices via fontdb) et des effets maison (flou gaussien par passes de boîte, pixellisation, ombre).
Le même code produit l'aperçu et le fichier exporté : pas d'écart entre plateformes. Si les performances manquent
(très grandes captures, glisser fluide), on rend les calques statiques en cache et on ne redessine que l'objet actif
avant d'envisager vello.

### Modèle de document rééditable

Options : (a) JSON dans un bloc `iTXt` du PNG exporté : un seul fichier, mais l'original nécessaire pour annuler un
flou y serait embarqué, et la suppression des métadonnées le perdrait ; (b) fichier annexe JSON à côté du PNG :
simple, mais deux fichiers à garder ensemble ; (c) **conteneur `.kropa`**.

**Choix (c)** : `.kropa` = ZIP (crate zip 9.5.1) contenant `document.json` (schéma versionné : `version`, taille,
échelle, liste d'objets typés avec style, ordre, verrou), `original.png` et `preview.png`. Le PNG exporté dans le
dossier de l'utilisateur reste un PNG ordinaire, **aplati** : flou et masque y sont irréversibles. Le `.kropa`, qui
contient l'original non caviardé, reste dans le dossier de données local et n'est jamais envoyé. Migrations de schéma
testées par des fichiers d'exemple de chaque version.

## 4. Vidéo et GIF (retirés, décision du 11/11/2026)

> Cette section est **retirée** : vidéo et GIF sortent du produit (voir `docs/decisions.md`). Elle est conservée
> comme historique ; ne rien en implémenter. Ne restent valables que les repères de licence plus bas, à lire sans
> la partie FFmpeg.

### Options comparées

| Option | Avantages | Maintenance | Risque principal |
| --- | --- | --- | --- |
| **ffmpeg-next 9.0.0** (liaison libav*, WTFPL) | Tous les codecs, un seul code | Élevé | Suit chaque version majeure de FFmpeg ; la licence du binaire dépend de la compilation ; liaison délicate sous macOS sans signature |
| **Binaire `ffmpeg` externe** (ffmpeg-sidecar 2.7.0) | Pas de liaison, la distribution fournit l'encodeur et en porte la licence | Faible | Il faut lui pousser des images brutes par tube (≈ 1 Go/s en 4K 30 i/s) ; présence et options variables selon la distribution |
| **GStreamer** (gstreamer-rs 0.25.4, MIT/Apache) | `pipewiresrc` reçoit directement le flux du portail (dmabuf, sans copie) ; encodeurs choisis à l'exécution (VA-API, x264, openh264) ; pause, audio, mux MP4 dans un même pipeline ; c'est la voie de l'enregistreur de GNOME | Moyen | Disponibilité des greffons selon la distribution ; diagnostic des pipelines |
| **AVFoundation** (objc2-av-foundation 0.3.2) + ScreenCaptureKit | Encodeur VideoToolbox matériel, H.264/HEVC sous licence Apple, aucune dépendance | Moyen | API Objective-C verbeuse ; macOS seulement |
| **Crates pures** : `gif` 0.14.2 + `imagequant` 4.4.1, gifski 1.34.0, rav1e 0.8.1, openh264 0.9.10 | Aucune dépendance système | Faible (GIF) | rav1e trop lent pour du temps réel ; openh264 compile le code Cisco sans la couverture de brevets du binaire Cisco ; **gifski est AGPL-3.0** |

### Choix

- **Linux : GStreamer** (portail ScreenCast → `pipewiresrc` → encodeur → `mp4mux`). Ordre de préférence, détecté à
  l'exécution : `vah264enc` (matériel) > `x264enc` > `openh264enc` ; à défaut **WebM VP9** (`vp9enc`, sans
  brevet). Un MP4 impossible est signalé, jamais simulé.
- **macOS : ScreenCaptureKit + AVAssetWriter** (H.264 VideoToolbox). Aucun FFmpeg embarqué.
- **GIF : Rust pur, commun** : images décimées, quantification `imagequant` (GPL-3.0-or-later, le moteur de gifski
  sans sa couche AGPL), écriture `gif`. Le binaire `gifski` externe peut rester un choix optionnel.
- **Coupe début/fin** : remux sans réencodage (GStreamer / `AVAssetExportSession`), à l'image clé la plus proche ;
  réencodage seulement si la précision est demandée.
- **FFmpeg n'est pas requis.** Un back-end « ffmpeg externe » reste possible plus tard (conversion, formats rares),
  en appelant le binaire du système.

### Licences et brevets : ce qu'il faut savoir

- FFmpeg est **LGPL-2.1-or-later** ; une compilation avec `--enable-gpl` (obligatoire pour libx264, GPL) devient
  **GPL** dans son ensemble ; `--enable-nonfree` rend le binaire non redistribuable (ffmpeg.org/legal.html).
- H.264 est couvert par des brevets (pool Via LA). Fedora livre `ffmpeg-free` **sans H.264** et passe par le
  binaire OpenH264 de Cisco, qui couvre les redevances ; certains dérivés (Bazzite) le désactivent ; le serveur de
  Cisco est injoignable dans certains pays. Le Freedesktop SDK a **retiré OpenH264** (25.08) au profit de l'extension
  Flatpak `codecs-extra` (x264/x265 compilés depuis les sources).
- Conséquence pour Kropa : **ne livrer aucun encodeur H.264** dans ses binaires. Sous Linux, l'encodeur vient du
  système (la distribution en porte la licence et le risque de brevet) ; sous macOS, d'Apple. Un Flatpak éventuel
  déclarera `codecs-extra`.

### Licence retenue : GPL-3.0-or-later

> La licence reste **GPL-3.0-or-later**, choix de l'utilisateur (aligné sur ShareX et Flameshot). La justification
> par FFmpeg, x264 et imagequant ci-dessous est caduque avec la vidéo ; seule la compatibilité avec les
> dépendances Apache-2.0 reste un argument technique.

| Licence | Pour | Contre |
| --- | --- | --- |
| MIT/Apache-2.0 | Réutilisation maximale | Interdit d'intégrer `imagequant` (GPL-3) ; une liaison future à un FFmpeg GPL serait impossible |
| MPL-2.0 | Copyleft par fichier | Même blocage pour imagequant et FFmpeg GPL |
| GPL-2.0-only | Classique | **Incompatible avec Apache-2.0** (xcap, rxing, russh, aws-sigv4…) et GPL-3 |
| **GPL-3.0-or-later** | Compatible Apache-2.0, LGPL et GPL-2.0-or-later (FFmpeg, x264), imagequant, Slint GPL ; protège un projet libre | Fork propriétaire impossible (voulu) |

La GPL-3 autorise aussi la combinaison avec l'AGPL-3 (article 13) si l'on reprend gifski un jour, mais on l'évite
par défaut. `cargo-deny` vérifie à chaque CI que toutes les licences de l'arbre restent compatibles GPL-3.

## 5. OCR et QR (local uniquement)

| Option | Avantages | Maintenance | Risque principal |
| --- | --- | --- | --- |
| **Apple Vision** (objc2-vision 0.3.2) | Local, précis, FR/EN, sans modèle à livrer | Faible | macOS seulement |
| **Tesseract, binaire externe** | Mûr, paquets `tesseract-ocr-fra/eng` partout | Faible | Dépendance optionnelle à détecter ; qualité moyenne sur captures d'écran à petit corps |
| **Tesseract lié** (leptess 0.14.0, inactif depuis 2023 ; tesseract-rs 0.4.0, 2026-07-31, jeune) | En processus | Moyen | leptess abandonné ; tesseract-rs compile Tesseract, chaîne C++ |
| **ocrs 0.14.0** (2026-11-08, MIT/Apache, moteur rten 0.28.0) | Pur Rust, actif | Faible | « Early preview », **alphabet latin seulement** ; télécharge ses modèles au premier lancement (à remplacer par des modèles livrés : requête réseau non configurée interdite) |

**Choix** : Apple Vision sous macOS ; **Tesseract externe** sous Linux pour la v1 (désactivé proprement s'il manque) ;
ocrs réévalué comme moteur intégré une fois les accents français vérifiés et les modèles livrés avec le paquet.
Le texte OCR alimente l'index de recherche de l'historique et la détection assistée de secrets (expressions
régulières de type gitleaks sur le texte et ses boîtes, crate `regex`).

**QR** : **rqrr 0.11.0** (pur Rust, QR seulement, léger) vs **rxing 0.9.4** (port de ZXing, tous les codes-barres,
plus lourd). Choix : rqrr, commun aux deux plateformes ; rxing si les codes-barres entrent au périmètre.

## 6. Structure du projet

### Processus : démon résident ou non

- **Sans démon** (chaque commande lance une interface) : simple, mais raccourcis globaux, file d'envoi avec reprise,
  miniature flottante après la capture et propriété du presse-papiers Wayland (perdue à la fermeture du processus)
  deviennent impossibles ou fragiles.
- **Avec démon** (choix) : l'application graphique *est* le démon (`GtkApplication` à instance unique sous Linux,
  agent `LSUIElement` avec `NSStatusItem` sous macOS). Elle possède raccourcis, presse-papiers, file d'envoi,
  fenêtres. La CLI `kropa` est un client léger ; sans démon, les actions sans interface (`upload`, `history`,
  `config`) tournent en processus, les autres démarrent le démon (activation D-Bus ou `open -a`).
- Deux binaires : `kropa` (CLI, **sans dépendance GTK**, utilisable sur un serveur pour envoyer des fichiers) et
  `kropa-app` (coquille graphique).

### IPC

Options : D-Bus (zbus 6.2.1) — natif Linux, absent de macOS ; gRPC — trop lourd ; **socket Unix + JSON-RPC 2.0
ligne par ligne** (serde_json, tokio `UnixStream`) — identique sur les deux systèmes. **Choix : socket Unix**, dans
`$XDG_RUNTIME_DIR/kropa/` (Linux) et `~/Library/Caches/Kropa/` (macOS, chemin < 104 octets), droits 0600,
protocole versionné (`hello` avec version). D-Bus reste utilisé côté Linux pour l'activation et les portails.

### Crates du workspace

| Crate | Rôle |
| --- | --- |
| `kropa-core` | Types, configuration, modèles de nom, chaîne d'actions, erreurs, types du protocole IPC |
| `kropa-doc` | Modèle de document, outils, annuler/rétablir, format `.kropa` |
| `kropa-render` | tiny-skia + cosmic-text : canevas, effets, habillage (fond, marge, ombre), export PNG/JPEG |
| `kropa-capture` | Trait `Capturer` ; modules `linux/` (portail, wlroots, X11) et `macos/` sous `cfg` |
| ~~`kropa-video`~~ | ~~Enregistrement (GStreamer / AVFoundation), GIF, coupe~~ **Retirée (décision du 11/11/2026)** |
| `kropa-vision` | OCR, QR, détection de secrets |
| `kropa-upload` | Destinations, import `.sxcu`, file persistante, reprise, envoi par morceaux |
| `kropa-store` | Historique SQLite, fichiers, secrets (trousseau + repli chiffré) |
| `kropa-i18n` | Ressources Fluent FR/EN partagées |
| `kropa-cli` | Binaire `kropa` (clap 4.6.7) |
| `kropa-gtk` / `kropa-macos` | Coquilles graphiques (binaire `kropa-app`) |

Dix crates au départ (onze avant le retrait de la vidéo), pas plus : capture Linux et macOS restent deux modules d'une même crate tant qu'ils
compilent séparément sans gêne. Le cœur ne dépend d'aucune bibliothèque graphique et se teste sans écran.

### Autres choix transverses

| Sujet | Options | Choix |
| --- | --- | --- |
| Asynchrone | glib seul ; **tokio** sur fils dédiés | Boucle native (glib, runloop AppKit) au fil principal, tokio 1.54.1 pour réseau et IPC, canaux entre les deux |
| Configuration | **TOML** (toml 1.2.1) ; JSON ; KDL | TOML : lisible, commentable, standard Rust. `config.toml` + `uploaders/*.toml` ; `.sxcu` converti en TOML à l'import ; chemins via `directories` 6.0.0 |
| Historique | **rusqlite 0.42.0** (SQLite embarqué, FTS5) ; redb ; fichiers JSON | SQLite : recherche plein texte sur nom, destination et texte OCR ; migrations versionnées |
| Secrets | **keyring 4.2.0** (Secret Service, Keychain) ; secret-service 5.2.0 seul | keyring ; repli **age 0.12.1** (fichier chiffré par phrase de passe, scrypt) plutôt qu'un assemblage maison argon2 + chacha20poly1305 |
| HTTP et envoi | reqwest 0.13.5 (rustls) ; aws-sdk-s3 1.159.0 (très lourd) ; rust-s3 0.38.0 ; opendal 0.59.5 | reqwest pour Zipline, Imgur, HTTP générique ; S3 par reqwest + **aws-sigv4** 1.6.5 (multipart maison, compatible MinIO/R2) ; SFTP par **russh 0.65.1 + russh-sftp 3.0.1** (pur Rust) |
| i18n | **Fluent** (fluent 0.17.0, i18n-embed 0.16.0) ; gettext-rs 0.8.0 | Fluent : mêmes fichiers `.ftl` pour CLI, GTK et AppKit, pluriels corrects ; chaînes posées depuis Rust, pas dans les `.ui` |
| Raccourcis globaux | **portail GlobalShortcuts** (ashpd) ; **global-hotkey** 0.10.0 ; raccourci du bureau → CLI | Portail sous Wayland (KDE ; GNOME 48+, fiabilité à vérifier) ; global-hotkey sous X11 et macOS ; repli documenté partout : raccourci personnalisé du bureau lançant `kropa capture area` |
| Presse-papiers | arboard 3.7.0 ; wl-clipboard-rs 0.9.5 ; API du toolkit | `GdkClipboard` (démon propriétaire de la sélection) et `NSPasteboard` (fichier + image + URL en même temps) ; wl-clipboard-rs pour la CLI sans démon là où data-control existe |
| Zone d'icône | ksni 0.3.7 (StatusNotifierItem) ; tray-icon 0.26.1 | ksni sous Linux (KDE natif ; GNOME seulement avec l'extension AppIndicator) ; `NSStatusItem` sous macOS |
| Métadonnées | img-parts 0.4.2, kamadak-exif 0.6.2 | Nos exports n'écrivent aucune métadonnée ; img-parts retire EXIF/XMP des fichiers importés avant envoi |

## 7. Version de Rust et politique de dépendances

- Rust stable au 11/11/2026 : **1.99.1** (canal stable, 2026-10-13). Édition **2024**.
- **MSRV : 1.95** (2026-04-16). Contraintes mesurées : aws-sigv4 1.94.1, rten 1.94, gtk4 1.92, rusqlite 1.89.
  Politique : MSRV ≥ 6 mois de recul, relevée au plus une fois par trimestre, vérifiée en CI (`cargo +1.95 check`).
  Le développement se fait sur la stable épinglée par `rust-toolchain.toml`.
- `Cargo.lock` commité ; dépendances déclarées dans `[workspace.dependencies]` ; fonctionnalités par défaut coupées
  quand c'est possible (rustls, pas OpenSSL).
- CI : `cargo deny` (licences compatibles GPL-3, sources, doublons), `cargo audit` (RustSec), `cargo machete`.
  Mises à jour groupées hebdomadaires (Renovate ou Dependabot), actions GitHub épinglées par SHA.
- Critères d'entrée d'une crate : release de moins d'un an ou dépôt actif, plus d'un mainteneur ou une organisation,
  licence compatible. Écartées pour cette raison : scap (inactif), leptess (inactif), mp4 0.14.0 (2023).
- Bibliothèques système admises sous Linux (GTK4, libadwaita, GStreamer, PipeWire) : versions minimales à fixer avec
  l'équipe Linux selon les distributions cibles.

## 8. Tableau récapitulatif

« Activité » = dernier commit sur la branche par défaut (GitHub) ou, à défaut, date de la dernière version.

| Crate | Version | Date version | Licence | Activité | Rôle |
| --- | --- | --- | --- | --- | --- |
| gtk4 | 0.11.5 | 2026-09-20 | MIT | 2026-11-10 | Coquille Linux |
| libadwaita | 0.9.2 | 2026-07-07 | MIT | (GitLab) version | Widgets GNOME |
| gtk4-layer-shell | 0.8.1 | 2026-08-10 | MIT | version | Overlay KDE/wlroots |
| objc2 | 0.6.5 | 2026-10-05 | MIT | 2026-10-17 | Liaisons Objective-C |
| objc2-app-kit / -vision / -av-foundation | 0.3.2 | 2025-10-04 | Zlib/Apache/MIT | 2026-10-17 | AppKit, OCR, vidéo macOS |
| ashpd | 0.14.0 | 2026-11-05 | MIT | 2026-11-08 | Portails XDG |
| pipewire | 0.10.1 | 2026-08-19 | MIT | (GitLab) version | Flux ScreenCast |
| libwayshot | 0.10.0 | 2026-11-08 | BSD-2 | 2026-11-08 | Capture wlroots |
| xcap | 0.9.10 | 2026-11-09 | Apache-2.0 | 2026-11-09 | Capture X11 |
| screencapturekit | 11.1.2 | 2026-11-02 | MIT/Apache | 2026-11-02 | Capture macOS |
| tiny-skia | 0.12.0 | 2026-02-02 | BSD-3 | 2026-08-20 | Rendu 2D |
| cosmic-text | 0.19.0 | 2026-04-22 | MIT/Apache | version | Texte des annotations |
| zip | 9.5.1 | 2026-11-10 | MIT | version | Conteneur `.kropa` |
| ~~gstreamer (+ -app, -video)~~ | 0.25.4 | 2026-09-21 | MIT/Apache | (GitLab) version | ~~Vidéo Linux~~ retirée |
| ~~gif~~ | 0.14.2 | 2026-04-09 | MIT/Apache | version | ~~Écriture GIF~~ retirée |
| ~~imagequant~~ | 4.4.1 | 2025-07-13 | GPL-3.0+ | version | ~~Palette GIF~~ retirée |
| rqrr | 0.11.0 | 2026-09-02 | MIT/Apache + ISC | 2026-09-02 | QR |
| ocrs (réserve) | 0.14.0 | 2026-11-08 | MIT/Apache | 2026-11-08 | OCR pur Rust, à réévaluer |
| rusqlite | 0.42.0 | 2026-11-11 | MIT | version | Historique |
| toml | 1.2.1 | 2026-11-06 | MIT/Apache | version | Configuration |
| keyring | 4.2.0 | 2026-08-29 | MIT/Apache | version | Trousseau |
| age | 0.12.1 | 2026-07-14 | MIT/Apache | version | Repli chiffré |
| reqwest | 0.13.5 | 2026-09-08 | MIT/Apache | version | HTTP |
| aws-sigv4 | 1.6.5 | 2026-11-09 | Apache-2.0 | version | Signature S3 |
| russh / russh-sftp | 0.65.1 / 3.0.1 | 2026-11-03 / 09-28 | Apache-2.0 | version | SFTP |
| tokio | 1.54.1 | 2026-10-21 | MIT | version | Asynchrone |
| fluent / i18n-embed | 0.17.0 / 0.16.0 | 2025-05 / 2025-07 | MIT/Apache | 2026-11-08 (cargo-i18n) | i18n |
| global-hotkey | 0.10.0 | 2026-11-09 | MIT/Apache | 2026-11-09 | Raccourcis X11/macOS |
| wl-clipboard-rs | 0.9.5 | 2026-10-26 | MIT/Apache | version | Presse-papiers CLI |
| ksni | 0.3.7 | 2026-10-20 | Unlicense | version | Tray Linux |
| clap | 4.6.7 | 2026-09-14 | MIT/Apache | version | CLI |
| img-parts | 0.4.2 | 2026-11-03 | MIT/Apache | version | Retrait EXIF |

Écartées (pour mémoire) : egui 0.37.0, iced 0.14.0, slint 1.18.1, tauri 2.12.5, relm4 0.11.0, vello 0.11.0,
skia-safe 0.157.0, femtovg 0.28.0, ffmpeg-next 9.0.0, ffmpeg-sidecar 2.7.0, gifski 1.34.0 (AGPL), scap 0.0.8,
leptess 0.14.0, rxing 0.9.4, aws-sdk-s3 1.159.0.

## 9. Hypothèses à confirmer par les équipes Plateformes

**Linux**
1. Le portail `Screenshot` non interactif est utilisable en boucle sur GNOME 47+ et KDE Plasma 6 après une première
   autorisation, sans boîte de dialogue à chaque capture (vérifier aussi hors Flatpak).
2. L'overlay plein écran sur image figée s'affiche correctement par moniteur sous GNOME (sans layer-shell), y compris
   avec mise à l'échelle fractionnaire mixte.
3. **Épinglage et miniature flottante sous GNOME Wayland** : ni « toujours au-dessus » ni positionnement possibles
   côté client. Repli à définir (menu « Toujours au premier plan » du bureau, notification avec actions, extension
   GNOME hors v1). Sur KDE et wlroots : layer-shell.
4. Portail GlobalShortcuts réellement fonctionnel sur GNOME 48/49 (rapport d'août 2026 : échec « already in use »)
   et sur KDE ; sinon le repli « raccourci du bureau → CLI » devient la voie principale sous GNOME.
5. Le démon garde la propriété du presse-papiers sous GNOME via `GdkClipboard` sans fenêtre focalisée ; et la CLI
   seule (wl-clipboard-rs) fonctionne-t-elle sous Mutter (protocole data-control) ?
6. Affichage des clics et touches pendant l'enregistrement : pas d'écoute globale des entrées pour un client
   Wayland. Probablement **impossible sous GNOME/KDE** sans droits sur `/dev/input` ; à confirmer et documenter.
7. Capture défilante : pas d'injection de défilement sous Wayland sans portail RemoteDesktop ; mode « l'utilisateur
   fait défiler, Kropa assemble » acceptable ?
8. Greffons GStreamer présents par défaut (Fedora, Ubuntu, Debian, openSUSE, Arch) : `pipewiresrc`, `mp4mux`,
   au moins un encodeur H.264 ; sinon repli WebM VP9.
9. Canaux de distribution sans AUR : Flatpak (avec `codecs-extra`), AppImage, paquets .deb/.rpm ? Impact du bac à
   sable Flatpak sur le socket IPC, le trousseau et l'appel au binaire `tesseract`.

**macOS**
10. Version minimale **macOS 14** acceptable (SCScreenshotManager) ; micro dans les vidéos seulement en 15+.
11. Signature ad hoc : l'autorisation « Enregistrement de l'écran » (TCC) est liée à la signature ; est-elle
    perdue à **chaque mise à jour** ? Fréquence des rappels d'autorisation sous macOS 15/26.
12. Gatekeeper : procédure « Ouvrir quand même » ou `xattr` à documenter. **Homebrew retire depuis le 1er
    septembre 2026 les casks qui échouent au contrôle Gatekeeper** : un tap personnel reste-t-il viable ?
13. Écoute globale des clics et touches (CGEventTap) : nécessite « Surveillance de l'entrée » ; même problème de
    réinitialisation à chaque mise à jour.
14. `global-hotkey` sous macOS sans autorisation d'accessibilité (API Carbon) ; `NSPanel` non activant pour la
    miniature et l'épingle ; `NSDraggingSource` depuis un panneau.

## 10. Questions ouvertes

1. **Coût de deux coquilles natives** pour un développeur seul : accepter que la coquille macOS suive la coquille
   Linux d'un lot, ou viser la parité à chaque lot ? (Le brief dit « qualité égale autant que possible ».)
2. Ordre de développement proposé : cœur + CLI, puis Linux GNOME, puis macOS, puis KDE/wlroots/X11. À valider.
3. OCR Linux : Tesseract externe suffit-il pour la v1, ou faut-il un moteur intégré (ocrs) malgré son statut ?
4. ~~Vidéo : MP4 obligatoire, ou WebM accepté comme repli ?~~ Sans objet : vidéo retirée.
5. ~~Back-end FFmpeg externe optionnel dès la v1 ?~~ Sans objet : vidéo retirée.
6. ~~La licence GPL-3.0-or-later convient-elle au porteur du projet ?~~ Tranché : oui.
7. Stockage des `.kropa` : dossier de données seulement, ou option « garder le projet à côté du PNG » ?
8. ~~Désaccord possible avec l'équipe Plateformes Linux sur GStreamer vs PipeWire + FFmpeg.~~ Sans objet : vidéo retirée.

## Sources (consultées le 2026-11-11)

- Versions, dates, licences, MSRV : `https://crates.io/api/v1/crates/<nom>` pour chaque crate citée.
- Rust stable : https://static.rust-lang.org/dist/channel-rust-stable.toml ; 1.95 :
  https://static.rust-lang.org/dist/channel-rust-1.95.0.toml
- Activité : dépôts GitHub clonés (dernier commit), p. ex. https://github.com/nashaofu/xcap,
  https://github.com/helmerapp/scap, https://github.com/doom-fish/screencapturekit-rs,
  https://github.com/iced-rs/iced, https://github.com/robertknight/ocrs, https://github.com/houqp/leptess
- xcap, état Wayland (⛔) et usage de `CGWindowListCreateImage` : README et `src/` de https://github.com/nashaofu/xcap
- screencapturekit-rs, versions macOS par fonction : README de https://github.com/doom-fish/screencapturekit-rs
- ocrs, « early preview », latin seulement : README de https://github.com/robertknight/ocrs
- iced 0.14 : https://github.com/iced-rs/iced/releases/tag/0.14.0 ; layer-shell :
  https://docs.rs/crate/iced_layershell/0.18.1/source/src/lib.rs
- Glisser-déposer winit et Slint : https://github.com/rust-windowing/winit/pull/4571 ,
  https://slint.dev/blog/slint-1.17-released
- GTK4 sous macOS : https://blog.gtk.org/2025/02/01/whats-new-in-gtk-winter-2025-edition/ ,
  https://discourse.gnome.org/tag/macos
- Toujours au-dessus sous Wayland : https://gitlab.freedesktop.org/wayland/wayland-protocols/-/merge_requests/369 ,
  https://discourse.gnome.org/t/gtk4-and-set-keep-above-and-other-similar-apis-that-used-to-be-tied-to-x11/12447
- GlobalShortcuts GNOME : https://thisweek.gnome.org/posts/2025/02/twig-189 ,
  https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/issues/85 ,
  https://newreleases.io/project/gnome-gitlab/GNOME/xdg-desktop-portal-gnome/release/50.alpha ,
  https://claudeissues.com/issue/89160-claude-desktop-linux-official-apt-package-quick-entry-global-shortcut-fails-when
- FFmpeg, licences : https://www.ffmpeg.org/legal.html
- H.264 et Fedora : https://discussion.fedoraproject.org/t/h-264-support-in-fedora-workstation-by-default/114521 ,
  https://discussion.fedoraproject.org/t/ciscobinary-openh264-org-is-unreachable-in-some-countries-ru-ua-ir/161434
- Freedesktop SDK sans OpenH264 : https://itsfoss.com/news/freedesktop-sdk-drops-openh264
- Homebrew et casks non notariés : https://workbrew.com/blog/homebrew-5-0-0 ,
  https://www.helpnetsecurity.com/2026/09/15/homebrew-7-0-0-security-open-source/
