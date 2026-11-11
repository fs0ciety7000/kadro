# Kropa — Plateforme Linux

> Proposition d'équipe du 11/11/2026. En cas de contradiction, docs/decisions.md prévaut.

> Équipe Plateforme Linux · état vérifié en ligne le 11 novembre 2026 · priorité GNOME et KDE sous Wayland.
> Références entre crochets `[n]` : liste des sources en fin de document. « Non vérifié » : je n'ai pas trouvé de source.

## 0. Contexte de novembre 2026 (ce qui change la donne)

- **GNOME 50 a supprimé la session X11** (Mutter n'est plus qu'un compositeur Wayland) ; GNOME 51 (16 septembre 2026)
  est la version courante [1][2]. **Plasma 6.8 (14 octobre 2026) a lui aussi abandonné la session X11** ; les correctifs
  X11 continuent sur Plasma 6.7 jusqu'au début de 2027 [3]. X11 ne concerne donc plus que Xfce, MATE, Cinnamon, i3,
  etc., et les applications XWayland.
- **xdg-desktop-portal** : 1.21.0 (janvier 2026) a ajouté `ConfigureShortcuts`, les jetons d'activation dans les
  signaux GlobalShortcuts et l'obligation d'un App ID valide ; 1.21.2 (mai 2026) a ajouté la **sélection de cible au
  portail Screenshot** (`target` : écran, fenêtre, zone, fenêtre active ; interface v3) ; 1.22.x (juin 2026) n'apporte
  que des correctifs [4][5][6].
- **Portails par bureau** : GNOME implémente Screenshot, ScreenCast, RemoteDesktop, InputCapture, GlobalShortcuts
  (depuis GNOME 48) et, depuis GNOME 51, le portail Secret via oo7 [7][2]. KDE implémente les mêmes, plus Secret (KF 6.2)
  [8][9]. `xdg-desktop-portal-wlr` n'implémente **que** Screenshot et ScreenCast [10] ; `xdg-desktop-portal-hyprland`
  ajoute GlobalShortcuts [11].
- **Protocoles Wayland** (tableaux de wayland.app, compositeurs Mutter 51, KWin 6.7, Sway 1.11, Hyprland 0.52, niri 26.04) :

| Protocole | Mutter | KWin | Sway | Hyprland | niri | Sources |
|---|---|---|---|---|---|---|
| ext-image-copy-capture-v1 | non | non | oui | non | non | [12] |
| wlr-screencopy (déprécié) | non | non | oui | oui | oui | [13] |
| wlr-layer-shell | **non** | oui (v5) | oui | oui | oui | [14] |
| ext-foreign-toplevel-list | non | non | oui | oui | oui | [15] |
| ext-data-control (presse-papiers) | **non** | oui | oui | oui | oui | [16] |
| xdg-toplevel-drag | oui | oui | non | non | non | [17] |
| xdg-toplevel-tag | oui | oui | non | oui | non | [18] |
| security-context-v1 (filtrage sandbox) | non | oui | oui | oui | oui | [19] |

- **Toujours au-dessus / position de fenêtre** : aucun protocole standard. La demande « always on top » de
  wayland-protocols (MR 369, valable seulement avant le premier commit) n'est pas fusionnée à ma connaissance (statut
  exact non vérifié) [20]. `xdg_toplevel` n'a aucune requête de position. Seul layer-shell (absent de Mutter) permet
  d'ancrer une surface dans un coin et au-dessus des fenêtres.
- **Captures d'écran directes** : GNOME réserve `org.gnome.Shell.Screenshot` à ses composants internes depuis GNOME 41
  et l'a même retiré à gnome-screenshot dans GNOME 49 [21][22]. KWin expose `org.kde.KWin.ScreenShot2`, réservé aux
  applications dont le `.desktop` déclare `X-KDE-DBUS-Restricted-Interfaces` (Flameshot le fait) [23][24].

## 1. Matrice de faisabilité

Légende : **P** possible · **D** dégradé · **I** impossible. Hypothèse : capture figée (voir § 3.1) puis sélection dans
notre propre overlay.

| Fonctionnalité | GNOME Wayland | KDE Wayland | wlroots (Sway, Hyprland) | X11 |
|---|---|---|---|---|
| Capture plein écran | **P** portail Screenshot ; permission demandée une fois | **P** portail, ou KWin ScreenShot2 en paquet natif | **P** screencopy direct (comme grim) | **P** XShm |
| Capture zone | **P** image figée + overlay | **P** idem | **P** idem (overlay layer-shell, comme slurp) | **P** |
| Écran actif | **D** pas de position globale du pointeur : on la déduit de l'entrée du pointeur dans l'overlay | **D** idem, ou ScreenShot2 en natif | **P** IPC Sway/Hyprland (`swaymsg`, `hyprctl`) ; sinon **D** comme GNOME | **P** |
| Loupe au pixel, dimensions | **P** dans l'overlay (image figée) | **P** | **P** | **P** |
| Fenêtre « propre » isolée, ombre, fond | **D** pas de liste de fenêtres ni de survol : choix dans le dialogue du portail (ScreenCast WINDOW ou Screenshot `target` fenêtre) | **D** en Flatpak (dialogue) ; **P** en natif via ScreenShot2 (fenêtre active ou interactive) | **P** Sway : foreign-toplevel + capture de toplevel ; **D** Hyprland : protocole propre, non vérifié | **P** XComposite |
| Capture différée | **P** minuterie puis capture (comportement d'un appel en arrière-plan non vérifié) | **P** | **P** | **P** |
| Capture défilante | **D** l'utilisateur fait défiler, nous assemblons le flux ScreenCast ; défilement auto via RemoteDesktop (2ᵉ permission) | **D** idem | **D** défilement manuel ; auto via wlr-virtual-pointer non vérifié | **P** XTest |
| Reprise de la dernière zone | **P** coordonnées mémorisées (invalidées si la disposition des écrans change) | **P** | **P** | **P** |
| Pipette | **P** dans l'overlay (le portail a aussi `PickColor`) | **P** | **P** | **P** |
| Règle en pixels | **P** overlay | **P** | **P** | **P** |
| OCR, QR | **P** traitement local, sans lien avec la plateforme | **P** | **P** | **P** |
| Vidéo écran | **P** ScreenCast + PipeWire, dialogue au 1ᵉʳ usage | **P** idem | **P** portail wlr ou hyprland | **P** XShm |
| Vidéo zone | **D** on recadre le flux de l'écran entier : dialogue écran + notre sélection | **D** idem | **D** idem | **P** |
| Vidéo fenêtre | **P** source WINDOW du portail (choix dans le dialogue) | **P** | **D** dépend du portail (wlr : non vérifié) | **P** |
| Audio système + micro | **P** PipeWire (moniteur de sortie + source) | **P** | **P** | **P** (PulseAudio/PipeWire) |
| Pause, reprise | **P** côté encodeur | **P** | **P** | **P** |
| Affichage du curseur | **P** curseur en métadonnées ScreenCast | **P** | **P** | **P** |
| Affichage des clics et touches | **I** sans assistant evdev ; **D** avec (paquet natif seulement) | **I / D** idem | **I / D** idem | **P** XInput2 |
| Miniature flottante dans un coin | **I** pas de position ni de layer-shell : **D** petite fenêtre placée par Mutter ou notification avec aperçu | **P** layer-shell ancré | **P** layer-shell | **P** |
| Glisser-déposer sortant | **P** depuis une fenêtre normale (`text/uri-list`) | **P** (depuis une surface layer-shell : non vérifié) | **P** (idem) | **P** XDND |
| Épinglage au-dessus | **I** par l'application : **D** fenêtre normale + « Toujours au premier plan » (menu Alt+Espace) à faire par l'utilisateur | **D** surface layer-shell « top » déplacée par nous, ou règle KWin | **D** layer-shell ; Hyprland : règle `pin` | **P** `_NET_WM_STATE_ABOVE` |
| Raccourcis globaux | **P** portail GlobalShortcuts (GNOME ≥ 48), validation par l'utilisateur | **P** portail (quelques bogues signalés) | **D** Hyprland : l'utilisateur lie `global` ; Sway : aucun portail, `bindsym` | **P** XGrabKey |
| Presse-papiers (image, lien) | **D** focus requis, pas de data-control ; lien copié après envoi incertain | **P** data-control (prévention du vol de focus : à tester) | **P** data-control | **P** (propriétaire vivant) |
| Trousseau Secret Service | **P** gnome-keyring ou oo7 ; portail Secret en Flatpak | **P** KWallet (≥ 5.97) ; portail en Flatpak | **D** dépend d'un fournisseur installé, sinon fichier chiffré | **D** idem |

## 2. Replis pour les cas dégradés ou impossibles

1. **Fenêtre propre sur GNOME (et KDE en Flatpak)** : pas de survol des fenêtres. On ouvre le dialogue du portail
   (ScreenCast `WINDOW`, une image, ou Screenshot `target`=fenêtre si le backend l'accepte, non vérifié). L'ombre et le
   fond sont ajoutés par Kropa sur l'image obtenue. Sur KDE natif : `ScreenShot2.CaptureActiveWindow` /
   `CaptureInteractive`. On l'écrit dans l'interface : « Sur GNOME, choisissez la fenêtre dans la fenêtre du système. »
2. **Écran actif** : on affiche l'overlay sur tous les écrans pendant quelques millisecondes ; l'écran qui reçoit
   `wl_pointer.enter` est l'écran actif. Si le pointeur ne bouge pas, l'événement peut manquer (non vérifié) : repli
   sur l'écran principal ou choix au clavier (1, 2, 3…).
3. **Capture défilante** : v1 en mode manuel (l'utilisateur fait défiler, nous assemblons les images d'un flux
   ScreenCast recadré sur la zone, détection du recouvrement). Défilement automatique en option : session RemoteDesktop
   + ScreenCast combinée (GNOME, KDE), donc une permission « contrôle à distance » en plus, impressionnante pour
   l'utilisateur. À ne pas proposer par défaut. Spectacle n'a toujours pas cette fonction [25].
4. **Vidéo d'une zone** : sélection dans notre overlay, puis le flux du moniteur concerné est recadré à l'encodage.
   Le dialogue du portail apparaît au premier usage seulement, si l'utilisateur coche « mémoriser » (jeton de
   restauration, § 4).
5. **Clics et touches** : Wayland ne permet pas d'observer les entrées globales. InputCapture ne convient pas (il
   capture les entrées, donc les retire au bureau, c'est un outil pour le partage clavier/souris). Repli v1 : surlignage
   du curseur à partir des métadonnées ScreenCast, **sans clics ni touches**. Option avancée (paquet natif, opt-in
   explicite) : petit assistant qui lit `/dev/input` via evdev, avec le groupe `input` ou polkit, comme Show Me The Key
   [26]. Il voit **toutes** les frappes (y compris les mots de passe) : à présenter comme tel, désactivé par défaut,
   impossible en Flatpak.
6. **Miniature flottante sur GNOME** : impossible de la placer dans un coin. Repli : (a) notification du portail
   Notification avec actions (Ouvrir, Copier, Annoter) ; (b) petite fenêtre normale, que Mutter place où il veut
   (souvent au centre) et qui peut prendre le focus. Recommandation : (a) par défaut sur GNOME, (b) en option.
7. **Épinglage** : GNOME, fenêtre normale sans décoration superflue et rappel « Alt+Espace → Toujours au premier
   plan ». KDE et wlroots : surface layer-shell (couche `top`) dont nous gérons nous-mêmes le déplacement par marges
   d'ancrage ; à défaut, documenter une règle KWin « Garder au-dessus » ciblant notre App ID (ou notre étiquette
   `xdg-toplevel-tag`). Même sur KDE, « garder au-dessus » n'est pas une garantie absolue [27].
8. **Raccourcis globaux hors portail** (Sway, river, autres wlroots ; ou refus de l'utilisateur sur GNOME/KDE) :
   l'utilisateur ajoute dans les réglages du bureau un raccourci qui exécute `kropa capture area` (ou `bindsym Print
   exec kropa capture area`). La CLI doit donc tout couvrir et répondre vite (daemon déjà lancé, § 3.4).
9. **Presse-papiers sur GNOME** : la copie Wayland exige le focus et un processus vivant qui sert les données. On
   copie l'image pendant que l'overlay a encore le focus. Pour le lien obtenu **après** l'envoi : notification
   « Lien prêt — Copier » (le clic donne un jeton d'activation donc le focus) ou petite surface éphémère, l'astuce de
   wl-copy, qui échoue parfois avec la prévention du vol de focus de Mutter [28]. On le dit dans l'interface au lieu de
   prétendre que le lien est copié.
10. **Trousseau absent** (wlroots, X11 minimal) : repli prévu par le brief (fichier chiffré par mot de passe maître).
    En Flatpak : le portail Secret ne donne qu'un secret maître propre à l'application ; on l'utilise pour chiffrer
    notre propre fichier, comme le fait libsecret en sandbox [29].

## 3. Choix techniques

### 3.1 Figer l'écran : portail Screenshot ou ScreenCast + PipeWire ?

| Critère | A. Screenshot non interactif | B. ScreenCast, une image du flux |
|---|---|---|
| Permission | une fois (permission store) sur GNOME ; KDE : non vérifié | dialogue de choix d'écran, puis jeton à usage unique renouvelé |
| Multi-écran | une image de tout le bureau ; répartition par écran à recalculer (échelles mixtes : non vérifié) | un flux par écran (`multiple`), géométrie fournie |
| Latence | faible, un appel D-Bus | démarrage du flux PipeWire (centaines de ms, non mesuré) |
| Curseur | non maîtrisé | au choix (caché, incrusté, métadonnées) |
| Réutilisable pour la vidéo | non | oui |

**Choix : A pour l'image fixe sur GNOME et KDE, B pour la vidéo et la capture défilante.** A est ce que font Flameshot
et les autres outils tiers sur GNOME [30]. Exceptions : KDE en paquet natif utilise ScreenShot2 (pas de dialogue,
capture de fenêtre, comme Flameshot et Spectacle) ; wlroots utilise directement ext-image-copy-capture ou wlr-screencopy
(comme grim) ; X11 utilise XShm. À valider par prototype : rendu multi-écran à échelles différentes avec A.

### 3.2 Overlay de sélection : layer-shell ou fenêtre plein écran classique ?

| Critère | A. layer-shell (couche `overlay`) | B. `xdg_toplevel` plein écran par écran |
|---|---|---|
| Disponibilité | KWin, wlroots, niri, COSMIC ; **pas Mutter** | partout |
| Au-dessus des panneaux et fenêtres « au-dessus » | oui | non garanti (Spectacle a ce bogue [27]) |
| Clavier exclusif | oui (`exclusive`) | si le focus est accordé (jeton d'activation requis) |
| Animations d'ouverture du bureau | aucune | possibles (à neutraliser, non vérifié) |

**Choix : A quand le protocole est annoncé, sinon B (GNOME, X11).** L'image figée rend les deux visuellement
identiques. Sur GNOME, l'activation passe par le jeton que fournit le signal `Activated` de GlobalShortcuts (1.21) [4]
ou par l'environnement de lancement (`XDG_ACTIVATION_TOKEN`).

### 3.3 Détection de l'environnement et stratégie par backend

| Option | Principe | Verdict |
|---|---|---|
| A. Par nom de bureau (`XDG_CURRENT_DESKTOP`) | table GNOME/KDE/Sway… | fragile (variables absentes, bureaux dérivés) |
| B. Par capacités | lire le registre Wayland (globals annoncés), les propriétés `version` et `AvailableSourceTypes` des portails, D-Bus (ScreenShot2 accessible ?) | robuste, testable |

**Choix : B, avec A seulement pour les messages d'aide** (« sur GNOME, faites… »). Ordre de préférence par
capacité : 1) API native autorisée (ScreenShot2, screencopy) ; 2) portail ; 3) X11 si `DISPLAY` sans `WAYLAND_DISPLAY`.
`kropa doctor` affiche les capacités détectées et les replis actifs.

### 3.4 Processus résident (daemon) ou CLI autonome ?

Un processus résident est **nécessaire** : servir le presse-papiers après la capture, garder la session
GlobalShortcuts, conserver les jetons de restauration, répondre vite à `kropa capture area` lancé par un raccourci du
bureau. Choix : daemon démarré à la session (autostart du portail Background en Flatpak, `.desktop` d'autostart en
natif) ; la CLI lui parle (D-Bus de session conseillé) et démarre en mode autonome s'il est absent.

## 4. Permissions et première utilisation

| Bureau | Ce que voit l'utilisateur |
|---|---|
| GNOME | 1) « Autoriser Kropa à faire des captures d'écran ? » une fois (permission store `screenshot`) ; un refus est mémorisé et doit être annulé à la main, piège connu de Flameshot [30]. 2) Dialogue GlobalShortcuts : l'utilisateur confirme ou modifie les raccourcis. 3) Première vidéo : choix écran/fenêtre, case « Mémoriser ce choix » (jeton de restauration, GNOME ≥ 42 [31]). |
| KDE | Dialogue GlobalShortcuts (réglages Plasma) ; dialogue ScreenCast avec « autoriser la restauration ». Des bogues de restauration ont été corrigés jusqu'en mai 2026 [32]. En natif, ScreenShot2 n'affiche aucun dialogue. |
| wlroots | Pas de dialogue pour screencopy (tout client non sandboxé peut capturer). Portail wlr : choix de la source par un sélecteur configurable (`man 5 xdg-desktop-portal-wlr`). Restauration depuis 0.7 [33]. |
| X11 | Aucun dialogue. |

Règles pour Kropa :
- **Jeton ScreenCast/RemoteDesktop** : demander `persist_mode=2`, stocker le jeton renvoyé par `Start()` et le
  **remplacer à chaque session** (usage unique) [34][35]. S'il est refusé (écran débranché, fenêtre fermée), le
  dialogue revient : prévoir ce cas.
- **Écran d'accueil** : expliquer avant chaque dialogue système ce qu'il va demander et pourquoi, pour que
  l'utilisateur ne refuse pas par réflexe. Bouton « Réinitialiser les autorisations » qui donne la commande
  (`flatpak permission-reset` ou suppression dans le permission store).
- **App ID** : les portails exigent désormais un App ID valide (GlobalShortcuts, 1.21) [4]. En Flatpak, il est
  automatique. En natif, installer `<app-id>.desktop` et s'enregistrer via l'interface hôte `Registry` (qui sera
  dépréciée un jour : gérer son absence) [36].

## 5. Packaging et sandbox (pas d'AUR)

| Fonctionnalité | Flatpak (Flathub) | Paquet natif (.deb, .rpm) | AppImage |
|---|---|---|---|
| Portails (Screenshot, ScreenCast, GlobalShortcuts) | **meilleur cas** : App ID fourni | OK si `.desktop` installé + Registry | **fragile** : pas de `.desktop` installé, App ID incertain |
| KWin ScreenShot2 (fenêtre propre sans dialogue) | non (non vérifié, très probablement refusé) | **oui** (`X-KDE-DBUS-Restricted-Interfaces`) | non |
| layer-shell, screencopy (KWin, Sway, Hyprland, niri) | **risque** : ces compositeurs filtrent les clients sandboxés via security-context ; liste exacte non vérifiée [19][37] | oui | oui |
| Assistant evdev (clics, touches) | **impossible** | possible (opt-in, polkit) | possible mais à installer à part |
| Audio système et micro | `--socket=pulseaudio` (PipeWire-Pulse) | oui | oui (bibliothèques de l'hôte) |
| Glisser-déposer sortant | chemin dans la sandbox : passer par le portail FileTransfer (non vérifié avec notre toolkit) | oui | oui |
| Secret Service | portail Secret (secret maître) | D-Bus direct | D-Bus direct |
| Mises à jour | Flathub | dépôts à maintenir (lourd pour un développeur seul) | manuelles |
| FFmpeg | runtime Freedesktop + extension codecs (à vérifier pour H.264) | dépendance de la distribution | à embarquer (licence, taille) |

**Recommandation** : Flatpak (Flathub) comme canal principal pour GNOME ; un paquet natif pour KDE et wlroots
complets (ScreenShot2, layer-shell, evdev). AppImage en dernier recours, avec mode dégradé annoncé par
`kropa doctor`. Pour un développeur seul : Flatpak + un seul format natif au départ (non tranché, voir questions).

## 6. Comment s'en sortent les autres outils

- **Flameshot** : portails sur GNOME (dialogue « Partager » à chaque fois sans permission préalable), ScreenShot2 sur
  KDE via son `.desktop`, grim sur Hyprland ; bricolages multi-écran sur Hyprland [30][24].
- **Spectacle** : ScreenShot2 et `CaptureScreen` (privilèges KWin) ; overlay plein écran avec bogues d'empilement ;
  ni capture défilante ni épinglage [23][25][27].
- **GNOME Screenshot** : mort sous Wayland depuis GNOME 49 ; l'outil intégré à GNOME Shell n'offre ni minuterie ni CLI
  [22].
- **Kooha** : portail ScreenCast + PipeWire + GStreamer, zone ou écran, audio système et micro ; enregistrement de
  fenêtre encore expérimental [38].
- **grim / slurp / swappy / satty** : wlroots uniquement ; slurp = overlay layer-shell, grim = screencopy, satty
  annote une image passée en entrée [39].
- **Ksnip** : comportement sous Wayland non vérifié (aucune source récente trouvée).

## 7. Points à transmettre

**Architecture**
- Un trait par capacité (`FreezeScreen`, `Overlay`, `WindowCapture`, `VideoSource`, `Shortcuts`, `Clipboard`,
  `Secrets`, `FloatingSurface`) et plusieurs implémentations choisies à l'exécution (§ 3.3). Jamais de « succès »
  renvoyé quand un repli est actif : l'API remonte le mode réel.
- Daemon obligatoire (§ 3.4). La CLI et les raccourcis du bureau passent par lui.
- Briques Rust probables : `ashpd` (portails), `pipewire`, `smithay-client-toolkit` (layer-shell, data-device),
  `x11rb`. Versions à choisir par l'équipe concernée, vérifiées au registre.
- Le toolkit d'interface doit permettre layer-shell **et** fenêtre classique, le glisser-déposer sortant et
  `xdg-activation` : critère d'élimination pour le choix du toolkit.
- Stockage persistant : jetons de restauration, dernière zone (avec empreinte de la disposition des écrans).

**Design**
- Premier lancement guidé avec explication avant chaque dialogue système.
- La miniature flottante n'existe pas sur GNOME : prévoir la variante notification.
- Épinglage et « fenêtre propre » se comportent différemment selon le bureau : textes d'aide spécifiques.
- Vidéo d'une zone : deux étapes visibles (dialogue système, puis notre sélection).
- Affichage des touches : désactivé par défaut, avertissement clair sur la lecture de toutes les frappes.
- Lien copié après l'envoi : prévoir l'état « Lien prêt — cliquer pour copier » (GNOME).

## 8. Questions ouvertes

1. Quels backends implémentent déjà `target` du portail Screenshot (1.21.2) ? Non vérifié pour GNOME et KDE.
2. Le portail Screenshot de KDE demande-t-il une permission en mode non interactif ? Non vérifié.
3. Rendu du portail Screenshot en multi-écran à échelles mixtes : prototype nécessaire.
4. Liste exacte des protocoles filtrés pour les clients Flatpak par KWin, Sway, Hyprland, niri (layer-shell ?
   screencopy ?) : à mesurer, cela décide du canal de distribution sur KDE.
5. Capture non interactive par un processus en arrière-plan (capture différée) acceptée par GNOME ? Non vérifié.
6. Acceptons-nous un assistant evdev privilégié (sécurité, packaging) pour les clics et touches ?
7. Une extension GNOME Shell (épinglage, miniature dans un coin) est-elle dans le périmètre ? Avis de l'équipe :
   non pour la v1 (maintenance à chaque version de GNOME).
8. Quel format natif en premier (.deb ou .rpm) ? Et quel niveau de support X11, maintenant que GNOME et Plasma n'ont
   plus de session X11 ?
9. Hyprland : capture de fenêtre par son protocole propre ou attendre ext-image-copy-capture ? Non vérifié.

## 9. État de l'implémentation (lot 1.4, `kropa capture screen|output`, `kropa doctor`)

> Ajout du 11/11/2026, lot 1.4. Ce qui est livré, ce qui est vérifié et ce qui reste à essayer sur un vrai bureau
> (`docs/testing/manual-linux.md`).

- **Moteur** : portail `org.freedesktop.portal.Screenshot` via `ashpd` 0.14.0 (zbus, runtime `async-io`, **sans tokio**),
  requête **non interactive** (`interactive=false`). Le portail décide : GNOME demande l'autorisation la première fois puis
  la mémorise ; le comportement de KDE n'est pas vérifié (question 2).
- **Identité** : `data/io.github.fs0ciety7000.Kropa.desktop` (installé par `scripts/dev-install.sh`) et enregistrement
  de l'identifiant auprès de l'interface hôte `org.freedesktop.host.portal.Registry` avant chaque requête. Un portail sans
  Registry (xdg-desktop-portal < 1.19) n'est pas une erreur : la capture continue, `kropa -v` le dit, et la permission
  n'est alors peut-être pas rattachée à l'identifiant.
- **Écran isolé** : impossible avec le portail seul (une image de tout le bureau) : `capture output` rend le code 5,
  raison `output_not_isolable`, avant toute attente. Aucun découpage n'est tenté.
- **Réponses du portail** : réponse 1 (fermée) et 2 (autre, ce que GNOME et KDE rendent après un refus) et l'erreur D-Bus
  `NotAllowed` donnent le code 4 (`portal_dismissed`, `portal_declined`, `portal_not_allowed`). La réponse 2 est
  indiscernable d'un échec du module de portail : le message le dit. Portail absent, interface absente, bus de session
  injoignable : code 10 (`portal_unavailable`, `no_session_bus`).
- **Image** : l'URI `file://` rendue par le portail est lue, vérifiée (structure et sommes de contrôle PNG), puis réécrite
  **sans métadonnées** (texte, EXIF, date, résolution) en gardant les blocs qui changent les couleurs ; la copie du portail
  est supprimée si elle est dans le dossier temporaire, laissée ailleurs. Format : PNG seul ; `jpeg` et `webp` rendent le
  code 5 (`image_format_unsupported`) avant toute capture.
- **Détection** : par capacités (bus de session, version de chaque interface de portail, cibles de la version 3,
  présence de KWin sur le bus, fichier `.desktop`). `XDG_CURRENT_DESKTOP` est affiché, jamais utilisé pour choisir.
  `layer-shell` et `data-control` ne sont pas encore sondés (`not_probed` dans `kropa doctor --json`) : il faut une
  connexion Wayland (lots 1.10 et 2.5).

## Sources

1. https://linuxiac.com/gnome-50-ends-the-x11-era-after-decades/ · https://fedoraproject.org/wiki/Changes/WaylandOnlyGNOME
2. https://www.omgubuntu.co.uk/2026/09/gnome-51-released · https://discourse.gnome.org/t/gnome-51-released/38820
3. https://phoronix.com/news/KDE-Plasma-6.8-Released · https://linuxiac.com/kde-plasma-6-8-will-go-fully-wayland-ending-nearly-30-years-of-x11-sessions/
4. https://raw.githubusercontent.com/flatpak/xdg-desktop-portal/main/NEWS.md
5. https://phoronix.com/news/xdg-desktop-portal-1.21
6. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html
7. https://release.gnome.org/48/developers/ · https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/issues/85
8. https://bugs.kde.org/show_bug.cgi?id=498575 (GlobalShortcuts KDE)
9. https://bugs.kde.org/show_bug.cgi?id=466197 (portail Secret KDE, KF 6.2)
10. https://github.com/emersion/xdg-desktop-portal-wlr
11. https://wiki.hypr.land/configuring/core/binds/globals/
12. https://wayland.app/protocols/ext-image-copy-capture-v1
13. https://wayland.app/protocols/wlr-screencopy-unstable-v1
14. https://wayland.app/protocols/wlr-layer-shell-unstable-v1
15. https://wayland.app/protocols/ext-foreign-toplevel-list-v1
16. https://wayland.app/protocols/ext-data-control-v1
17. https://wayland.app/protocols/xdg-toplevel-drag-v1
18. https://wayland.app/protocols/xdg-toplevel-tag-v1
19. https://wayland.app/protocols/security-context-v1
20. https://gitlab.freedesktop.org/wayland/wayland-protocols/-/merge_requests/369 · https://gitlab.freedesktop.org/wayland/wayland/-/issues/305
21. https://bugzilla.mozilla.org/show_bug.cgi?id=1835707
22. https://www.omgubuntu.co.uk/2025/10/gnome-screenshot-app-no-longer-works-in-gnome-49
23. https://lxr.kde.org/source/plasma/kwin/src/plugins/screenshot/org.kde.KWin.ScreenShot2.xml
24. https://github.com/flameshot-org/flameshot/blob/master/data/desktopEntry/package/org.flameshot.Flameshot.desktop
25. https://bugs.kde.org/show_bug.cgi?id=450719
26. https://hackweek.opensuse.org/projects/a-screenkey-alternative-that-works-under-wayland-via-reading-evdev-directly
27. https://bugs.kde.org/show_bug.cgi?id=500467
28. https://man.archlinux.org/man/wl-clipboard · https://bugs.kde.org/show_bug.cgi?id=511840
29. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Secret.html
30. https://flameshot.org/docs/guide/wayland-help/
31. https://issues.redhat.com/browse/RHEL-4526 · https://feaneron.com/2021/11/13/adventures-with-portals/
32. https://bugs.kde.org/show_bug.cgi?id=517454 · https://bugs.kde.org/show_bug.cgi?id=480235
33. https://salsa.debian.org/swaywm-team/xdg-desktop-portal-wlr/-/tags/v0.7.0
34. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
35. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html
36. https://flatpak.github.io/xdg-desktop-portal/docs/api-reference.html
37. https://niri-wm.github.io/niri/Security-Model.html · https://phoronix.com/news/Flatpak-Wayland-Security-Ctx
38. https://github.com/SeaDve/Kooha
39. https://nickjanetakis.com/blog/wayland-compatible-annotated-screenshots-with-slurp-grim-and-satty
