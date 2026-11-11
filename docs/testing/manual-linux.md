# Tests manuels Linux : capture par le portail Screenshot (lot 1.4)

> Ce que la CI ne peut pas prouver. La CI (`crates/kropa-cli/tests/portal.rs`) lance un vrai `dbus-daemon` privé et un
> **portail factice** : elle vérifie la conversation D-Bus (Registry, requête `Screenshot`, signal `Response`), le
> mappage de chaque réponse vers un code de sortie, la lecture de l'URI `file://`, le retrait des métadonnées PNG et
> l'enregistrement du fichier. Elle ne dit rien du comportement de GNOME, de KDE ou de wlroots : c'est l'objet de cette
> liste. Tout ce qui est coché ici l'est à la main, sur un vrai bureau, et le résultat se note à la fin.

## Avant de commencer (tous les bureaux)

- [ ] Build : `cargo build -p kropa` puis `scripts/dev-install.sh` (installe `io.github.fs0ciety7000.Kropa.desktop`
  dans `~/.local/share/applications`, sans sudo ; le relancer ne change rien).
- [ ] `kropa doctor` s'exécute, code 0, dans les deux langues (`--lang fr`, `--lang en`) et en `--json`.
  Noter : bureau annoncé (`XDG_CURRENT_DESKTOP`, indicatif), version de l'interface Screenshot, cibles
  (`portal_screenshot_targets`, version 3 seulement), `portal_registry`, `desktop_file` (doit valoir « oui »
  après `dev-install.sh`).
- [ ] Hors session graphique (ssh sans `DBUS_SESSION_BUS_ADDRESS`, ou `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
  kropa capture screen`) : code 10, `error.reason` `no_session_bus`, message qui dit quoi faire. Aucun fichier créé.
- [ ] `kropa capture output` : code 5, `output_not_isolable`, message qui renvoie vers `capture screen`, **sans** boîte de
  dialogue ni attente (essayer avec `--delay 30` : l'erreur doit être immédiate).

## GNOME (Wayland, Mutter, xdg-desktop-portal-gnome)

Version de GNOME et de `xdg-desktop-portal` à noter.

Première capture :

- [ ] `kropa capture screen --output /tmp/k.png` : une boîte « Autoriser Kropa à faire des captures d'écran ? » apparaît,
  avec le **nom « Kropa »** (preuve que le `.desktop` et le Registry sont pris en compte). Noter ce qui s'affiche si
  `dev-install.sh` n'a pas été lancé.
- [ ] Après « Autoriser » : fichier créé, code 0, chemin absolu sur stdout, `Enregistré : …` sur stderr. Ouvrir l'image :
  tous les écrans présents, bonne résolution, **curseur** présent ou non (noter).
- [ ] Seconde capture : aucune boîte, capture immédiate (permission mémorisée).
- [ ] `kropa -v capture screen …` : la ligne « Enregistré auprès du portail sous l'identifiant … » apparaît (Registry
  accepté). Noter si la note « pas de registre d'applications » apparaît à la place.
- [ ] Le fichier de l'image ne contient pas de métadonnées : `exiftool k.png` (ou `identify -verbose`) ne montre ni
  texte, ni date, ni logiciel, ni résolution en points par pouce.
- [ ] Où le portail a-t-il écrit **sa propre copie** ? (`~/Pictures`, `/tmp`, `/run/user/UID/doc/…`.) Kropa ne supprime
  que les copies placées dans le dossier temporaire ; noter s'il reste une copie plein écran ailleurs.

Refus :

- [ ] Réinitialiser (`flatpak permission-remove screenshot screenshot io.github.fs0ciety7000.Kropa`, ou
  `flatpak permission-reset io.github.fs0ciety7000.Kropa`), relancer, cliquer « Refuser » : **code 4**,
  `error.reason` = `portal_declined` ou `portal_dismissed` ou `portal_not_allowed` (noter lequel), aucun fichier.
- [ ] Relancer sans réinitialiser : le refus est mémorisé, code 4 immédiat sans boîte (comportement connu de Flameshot).
  Le message indique comment lever le refus.
- [ ] Fermer la boîte avec Échap : noter le code et la raison.

Multi-écran :

- [ ] Deux écrans, même échelle : l'image a la largeur cumulée, `file.width`/`file.height` du JSON correspondent.
- [ ] Deux écrans à échelles différentes (1× et 2×) : noter la taille de l'image et si la netteté de l'écran 2× est
  conservée (question ouverte 3 de `platform-linux.md`).

Cas particuliers :

- [ ] `--delay 5` : la boîte et la capture ont lieu **après** 5 s (question 5 : un appel après attente est-il accepté ?).
- [ ] Lancement depuis un raccourci du bureau (réglages du clavier) plutôt que depuis un terminal.
- [ ] Lancement depuis un terminal GNOME Console ou ptyxis (le Registry/cgroup change l'identité vue par le portail).

## KDE Plasma (Wayland, KWin, xdg-desktop-portal-kde)

Version de Plasma et de `xdg-desktop-portal-kde` à noter.

- [ ] `kropa doctor` : `kwin_service` = oui ; noter la version Screenshot et les cibles.
- [ ] `kropa capture screen --output /tmp/k.png` **non interactif** : le portail KDE prend-il la capture seul, après une
  demande d'autorisation, ou ouvre-t-il sa propre interface (type Spectacle) ? Question ouverte 2 de `platform-linux.md`.
  Si une interface s'ouvre, la décrire : elle est alors interactive malgré `interactive=false`, et le texte
  « non interactif si le portail le permet » devient « dépend du bureau ».
- [ ] Autorisation mémorisée entre deux captures ? Où la révoquer (Réglages > Confidentialité > Permissions ?) ?
- [ ] Refus : code 4, raison notée.
- [ ] Même contrôle des métadonnées, de la copie du portail, du multi-écran et de `--delay 5` que sous GNOME.
- [ ] `kropa capture output` : code 5 comme ailleurs (le portail ne sait pas isoler un écran).

## wlroots (Sway, Hyprland) avec xdg-desktop-portal-wlr

- [ ] `kropa doctor` : Screenshot présent ; ScreenCast présent ; GlobalShortcuts, Secret absents (`absent`).
- [ ] `kropa capture screen` : fonctionne-t-il (le portail wlr utilise `grim`) ? Sans `grim` installé : code 10 ou 4 ?
  Noter code et message (un échec du backend arrive sous la forme « réponse 2 », donc code 4 `portal_declined` : le
  message le dit-il assez clairement ?).
- [ ] Sans portail du tout (`xdg-desktop-portal` arrêté) : code 10, `portal_unavailable`, le message cite l'installation
  de `xdg-desktop-portal` et du module du bureau.

## Presse-papiers (lot 1.10)

La CI (`crates/kropa-cli/tests/clipboard_wayland.rs`) lance `sway --headless` (wlroots, `zwlr_data_control_v1`) et
`weston --backend=headless` (aucun protocole data-control, donc comme GNOME pour le presse-papiers), fait copier par le
vrai binaire `kropa` et relit avec `wl-paste`. Elle ne prouve rien pour Mutter, KWin ni `ext-data-control-v1`. À la
main, sur un vrai bureau, avec un serveur de test ou `kropa upload FICHIER --to PROFIL --copy markdown` :

- [ ] `kropa doctor` : ligne « Wayland data-control » = oui (KDE, wlroots) ou introuvable (GNOME). Noter le bureau.
- [ ] **KDE Plasma (Wayland)** : `kropa upload … --copy url` : code 0, `Link copied (url)`, `clipboard.status` =
  `copied` en `--json`. Coller dans un éditeur de texte **après** le retour de la commande : l'URL apparaît. Vérifier
  `ps -ef | grep __clipboard-serve` : un processus, qui disparaît dès qu'on copie autre chose. Plasma 6.x : noter si
  Klipper garde une entrée d'historique (attendu : oui).
- [ ] KDE : `kropa capture screen --copy image` : coller dans GIMP ou un client de messagerie : l'image est celle du
  fichier. Essayer aussi `--copy markdown` / `html` avec `--upload` et coller dans un champ de texte riche et dans un
  éditeur (le texte brut du lien, pas du HTML rendu).
- [ ] **GNOME (Wayland)** : même commande avec `--copy url` : **code 0**, URL brute seule sur stdout,
  `Link not copied: This Wayland session does not allow clipboard access without focus (GNOME does not). The link is
  printed above.` sur stderr, `clipboard.status` = `unavailable`, `reason` = `no_data_control`, aucun processus
  `__clipboard-serve`. Coller : l'ancien contenu est intact. Même chose en français (`--lang fr`).
- [ ] GNOME : `--copy image` après `capture screen` : `Image not copied: …`, code 0, fichier enregistré.
- [ ] **wlroots réel (Sway, Hyprland)** : comme KDE ; avec un gestionnaire d'historique (`cliphist`, `clipman`) noter s'il
  voit l'entrée. Sway ≥ 1.11 / wlroots ≥ 0.19 annonce `ext-data-control-v1` : noter lequel est utilisé (indifférent
  pour l'utilisateur).
- [ ] Un shell qui capture la sortie ne reste pas bloqué : `url=$(kropa upload … --copy url); echo "$url"` rend la main
  tout de suite, alors que le fils sert encore.
- [ ] Session X11 seule (`DISPLAY` sans `WAYLAND_DISPLAY`) : `clipboard.status` = `unavailable`,
  `reason` = `x11_not_supported`, code 0 (limite connue du lot 1.10).
- [ ] Sans session graphique (ssh nu) : `unavailable` / `no_graphical_session`, code 0.
- [ ] Fermer la session (ou tuer le compositeur) : le processus `__clipboard-serve` disparaît seul.

## Résultat

| Bureau | Version | Date | Testeur | Écarts avec le comportement attendu |
|---|---|---|---|---|
| GNOME | | | | |
| KDE Plasma | | | | |
| wlroots | | | | |

Presse-papiers : noter aussi, par bureau, `copied` / `unavailable` / `failed` obtenu et le protocole utilisé.

Reporter dans `docs/platform-linux.md` § 8 les questions 1 (cibles), 2, 3 et 5 dès qu'elles ont une réponse, et dans
`docs/plan.md` la question 2 de « Risques » (portail appelé depuis une CLI sans App ID).
