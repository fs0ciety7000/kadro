# Kropa — identité de marque (A « Repère »)

> Identité retenue par l'utilisateur le 11/11/2026 : **A « Repère »**, nom **kropa**. Aperçu visuel :
> [`preview.html`](preview.html) (à ouvrir dans un navigateur). Historique : nom de travail « Kadro » jusqu'au
> 11/11/2026, abandonné (voir [`naming.md`](naming.md)) ; l'identité B est archivée (section 9).

## 1. Positionnement

**Promesse.** Kropa capture, annote et partage votre écran sans quitter le clavier, et vos images restent chez vous
jusqu'au serveur que vous avez choisi.

- **Personnalité** : précis (au pixel, au raccourci près) ; sobre (aucun effet gratuit, aucune fanfare) ; loyal (dit
  ce qu'il fait, ce qu'il envoie, et ce qui a échoué).
- **Kropa n'est pas** un service en ligne, un réseau social de captures, un outil « magique » qui cache ce qu'il fait,
  ni une usine à options exposées d'emblée.
- **Le nom** : *kropa* évoque *crop* (recadrer) et les traits de coupe d'imprimerie, que dessinent précisément les
  deux équerres du logo ; en polonais, augmentatif de *kropka*, « point ». Écriture : `kropa` en bas de casse dans le
  logo, la commande, le crate et les chemins ; « Kropa » dans le texte courant.

## 2. Ton de voix

Règles : phrase courte, sujet clair, plutôt télégraphique ; on dit **ce qui s'est passé, où est le fichier, quoi
faire** ; pas de « Oups », pas de point d'exclamation, pas d'humour dans les erreurs ; on montre la commande CLI
équivalente quand elle aide ; vouvoiement en français ; mêmes termes partout (capture, envoi, destination,
historique).

| Situation | FR | EN |
|---|---|---|
| Notification « lien copié » | **Lien copié** · `https://img.maison.lan/u/a3F9k.png` — Enregistrée dans ~/Images/Kropa. [Ouvrir] [Annoter] | **Link copied** · `https://img.maison.lan/u/a3F9k.png` — Saved to ~/Pictures/Kropa. [Open] [Annotate] |
| Erreur d'envoi | **Envoi échoué** : Zipline a refusé le jeton (401). La capture est enregistrée en local et reste dans la file d'attente. [Réessayer] [Voir le journal] | **Upload failed**: Zipline rejected the token (401). The capture is saved locally and stays in the queue. [Retry] [View log] |
| Erreur réseau (variante) | **Envoi en attente** : `img.maison.lan` ne répond pas. Nouvel essai dans 30 s. | **Upload pending**: `img.maison.lan` is not responding. Retrying in 30 s. |
| Historique vide | Aucune capture pour l'instant. Appuyez sur Impr. écran ou lancez `kropa capture area`. | No captures yet. Press Print Screen or run `kropa capture area`. |
| À propos | Kropa : capture, annotation et envoi vers votre serveur. Rien ne quitte la machine sans votre accord. | Kropa: capture, annotate and upload to your own server. Nothing leaves the machine unless you say so. |

Dans l'application, le texte d'interface utilise **la police du système** (Adwaita Sans / Cantarell sous GNOME,
SF Pro sous macOS), par cohérence avec le principe « natif ». Les polices de marque servent au site, à la
documentation, au README et aux visuels.

## 3. Concept « Repère »

La zone éclairée : quand on sélectionne une zone, l'écran s'assombrit et seule la sélection reste claire. Le logo
garde deux équerres opposées (le cadre, les traits de coupe) et la **poignée** ambre qu'on tire au coin inférieur
droit (le geste). Pas d'appareil photo, pas d'objectif, pas de viseur centré.

## 4. Logo

Fichiers : `a/logo.svg` (clair), `a/logo-inverse.svg` (pour fond sombre `#111318`, fond transparent : le fond n'est
pas incrusté), `a/logo-mono.svg` (une couleur, `fill`/`stroke` en `currentColor` : il prend la couleur du texte
quand il est incorporé en ligne, noir quand il est chargé en `<img>`).
Symbole + mot-symbole « kropa » en bas de casse monolinéaire géométrique, terminaisons coupées net.

**Construction du mot-symbole** (unités du `viewBox="0 3 79 24"`, mot décalé de 23 unités) :
- Hauteur d'x 10 (y 10 → 20), trait 2, coupe nette (`butt`, `miter`). Le symbole va de l'ascendante à la ligne de
  base (y 4 → 20).
- `k`, `r`, `o`, `a` repris de l'ancien dessin. Les extrémités obliques du `k` sont coupées à l'horizontale par un
  `clipPath` : la jambe (`M4.3 13 8.4 20`) s'arrête sur la ligne de base (y ≤ 20), le bras sur la hauteur d'x
  (y ≥ 10) ; sans cela, la coupe `butt` perpendiculaire au trait dépassait de 0,54 unité. Le `p`, seule descendante, est un fût et la panse de l'`o` : son fût
  (y 10 → 26) a la même longueur que celui du `k` (y 4 → 20), descendante = ascendante = 6.
- Approches : 2 à 2,5 unités entre formes (2 entre les panses de `p` et `a`) ; 2,07 au plus près entre `r` et `o`
  (de l'angle bas de la terminaison du `r` à la panse de l'`o`), la terminaison s'arrêtant à 1,4 unité de l'aplomb
  de l'`o`. Centrage optique : le vide sous la terminaison du `r` élargit l'espace perçu, d'où cette approche plus
  serrée à l'horizontale, qui paraît égale aux autres. Le mot fait 55 unités, comme l'ancien.
- La zone du logo est l'encombrement du dessin plus 1 unité ; la descendante dépasse sous la ligne de base du
  symbole : aligner le logo sur sa ligne de base, pas sur le centre de sa boîte.

**Zone de protection** : la largeur de la poignée tout autour (4,7 unités, ≈ 1/3 de la hauteur du symbole).

**Taille minimale** (rendu Chromium headless, 1x et 2x, clair et sombre, hauteur de la boîte du logo) :

| Hauteur | Largeur | Trait | Constat |
|---|---|---|---|
| 32 px | 105 px | 2,7 px | Net partout. |
| 24 px | 79 px | 2 px | Net en 1x ; contreformes de `o`, `p`, `a` ouvertes (6 px) ; descendante bien visible. **Minimum.** |
| 16 px | 53 px | 1,3 px | Flou en 1x (traits sur deux pixels) ; équerre basse et poignée se fondent. Net en 2x. **Interdit en 1x, toléré si `devicePixelRatio` ≥ 2.** |

Minimum : **24 px de haut, soit une hauteur d'x de 10 px** (la hauteur d'x vaut 10/24 de la boîte) ; 16 px de haut
(hauteur d'x 6,7 px) seulement si `devicePixelRatio` ≥ 2. En dessous : le **symbole seul** (`a/icon-symbolic.svg`,
dessiné pour 16 px).

**Couleurs du logo** : trait `#15171C` sur clair (16,73:1 sur `#F6F7F9`), `#ECEEF2` sur sombre (16,00:1 sur
`#111318`). La poignée ambre fait 2,20:1 sur le fond clair. Les logos sont exemptés du critère de contraste des
objets graphiques (WCAG 1.4.11), mais le dessin ne compte pas sur cette exemption : la poignée se distingue par sa
**forme** (carré plein accolé à l'équerre sombre), pas par sa couleur, et le logo reste identifiable en une couleur. **Une couleur** : la poignée passe à la couleur du trait (`a/logo-mono.svg`) et reste
lisible grâce à son épaisseur.

## 5. Icônes

- `a/icon.svg` : macOS, gabarit 1024 (plaque superellipse 824 px, ombre incluse), tracée en 8 courbes de Bézier
  (écart à la superellipse n = 5 inférieur à 0,12 px). Plaque graphite, zone éclairée `#4A5160 → #3A404C`
  (1,92:1 en haut et 1,71:1 en bas sur la plaque, visible à 64 px), équerres blanches, poignée ambre.
- Linux (freedesktop/GNOME), grille 128, **sans plaque imposée** : carte paysage avec épaisseur en bas (convention
  Adwaita). Le glyphe est centré sur la face (`translate(20 16)`, marges haut et bas égales, 11 px) ; la poignée est
  agrandie à 6,5 unités (+40 % de surface). Deux variantes, **choix en attente** (planche de comparaison dans
  `preview.html`, contrastes WCAG mesurés) :
  - `a/icon-linux.svg`, **ardoise** : face `#7A8293 → #666E7E`, liseré blanc 25 %, épaisseur `#3A3F4A`. Face
    ≥ 3:1 sur `#111318` (3,62) et `#242424` (3,03) ; 2,57 sur `#303030`, où le liseré prend le relais (4,22).
    Coût : poignée à 2,17:1 sur la face et 2,14:1 contre l'équerre blanche.
  - `a/icon-linux-light.svg`, **claire** : face Light 2 → Light 3 (`#F6F5F4 → #DEDDDA`), épaisseur Light 5
    `#9A9996`, liseré noir 12 %, zone éclairée blanche, équerres `#241F31`. Face ≥ 9,7:1 sur tout fond sombre ;
    poignée 6,77:1 contre l'équerre qu'elle touche. Sur fond clair, la face se fond (1,30:1) comme les icônes
    Adwaita claires ; l'épaisseur détache le bas (2,73:1).
  L'ancienne plaque sombre (`#2B303A → #171A20`) tombait à 1,07:1 sur `#111318` et ressemblait aux icônes de
  terminal : abandonnée.
- `a/icon-symbolic.svg` : 16 × 16, aplats seulement (pas de trait, pour que GTK recolore), `#2e3436` ; alignés au
  pixel. Sur macOS, l'exporter en image *template* (PDF ou PNG noir + alpha, nom en `…Template`).

Les icônes ne contiennent pas de texte : seul leur `<title>` est passé à « Kropa ».

## 6. Palette

#### clair

| Jeton | Hex | Rôle | Contraste mesuré |
|---|---|---|---|
| `bg` | `#F6F7F9` | Fond de fenêtre | — |
| `surface` | `#FFFFFF` | Panneaux, barre d'outils, miniature | — |
| `line` | `#D9DCE2` | Séparateurs (décoratif) | 1.37:1 sur surface (décoratif) |
| `line-strong` | `#7D8490` | Contours de champs, outils inactifs | 3.77:1 sur surface (OK graphique (≥ 3)); 3.51:1 sur bg (OK graphique (≥ 3)) |
| `text` | `#15171C` | Texte principal | 17.93:1 sur surface (AAA); 16.73:1 sur bg (AAA) |
| `text-muted` | `#5A606B` | Texte secondaire, raccourcis | 6.32:1 sur surface (AA); 5.90:1 sur bg (AA) |
| `accent` | `#FF8A1F` | Ambre : remplissage d'action (outil actif, bouton principal), poignée | 7.88:1 sur texte #111318 posé dessus (AAA); 2.20:1 sur bg (jamais seul) (insuffisant seul) |
| `accent-text` | `#A84B00` | Liens, texte d'accent, anneau de focus | 5.72:1 sur surface (AA); 5.34:1 sur bg (AA) |
| `success` | `#1A7F45` | Succès (« Lien copié ») | 5.04:1 sur surface (AA) |
| `danger` | `#C62828` | Erreur | 5.62:1 sur surface (AA) |

#### sombre

| Jeton | Hex | Rôle | Contraste mesuré |
|---|---|---|---|
| `bg` | `#111318` | Fond de fenêtre | — |
| `surface` | `#1A1D24` | Panneaux, barre d'outils, miniature | — |
| `raised` | `#262A33` | Survol, éléments en relief | — |
| `line` | `#3A3F4A` | Séparateurs (décoratif) | 1.60:1 sur surface (décoratif) |
| `line-strong` | `#7A818D` | Contours de champs, outils inactifs | 4.30:1 sur surface (OK graphique (≥ 3)) |
| `text` | `#ECEEF2` | Texte principal | 14.52:1 sur surface (AAA); 12.37:1 sur raised (AAA) |
| `text-muted` | `#A2A8B3` | Texte secondaire, raccourcis | 7.06:1 sur surface (AAA); 6.01:1 sur raised (AA) |
| `accent` | `#FF8A1F` | Ambre : remplissage d'action, texte d'accent, focus | 7.15:1 sur surface (AAA); 7.88:1 sur texte #111318 posé dessus (AAA) |
| `success` | `#4ADE80` | Succès | 9.68:1 sur surface (AAA) |
| `danger` | `#FF6B6B` | Erreur | 6.08:1 sur surface (AA) |


Règles : l'ambre ne porte jamais seul une information en mode clair (2,20:1 sur le fond) ; il sert de
**remplissage** avec du texte `#111318` dessus (7,88:1). Pour un trait ou un texte d'accent en clair : `accent-text`.

## 7. Typographies

Licence libre, OFL 1.1.
- Titres et site : **Space Grotesk** — [github.com/floriankarsten/space-grotesk](https://github.com/floriankarsten/space-grotesk), [Google Fonts](https://fonts.google.com/specimen/Space+Grotesk).
- Code, CLI, raccourcis, URL : **JetBrains Mono** — [github.com/JetBrains/JetBrainsMono](https://github.com/JetBrains/JetBrainsMono), [Google Fonts](https://fonts.google.com/specimen/JetBrains+Mono). Distingue nettement 0/O, 1/l/I.

**Principes d'icône (barre d'outils, interface).** Grille 20 px, trait 1,75 px, extrémités carrées et angles vifs,
formes géométriques simples ; un seul accent (ambre) réservé à l'outil actif ; **lettre du raccourci visible sous
chaque outil**. Pour les actions standard (annuler, copier…), utiliser les icônes du système (Adwaita symbolic,
SF Symbols) ; les icônes maison ne couvrent que les outils propres à Kropa.

**Points à surveiller.** Confusion avec une icône de terminal : vérifiée dans le dock GNOME (planche de
`preview.html`), à refaire dans le Launchpad ; garder la poignée pleine dans toutes les déclinaisons ; le symbolique
reste proche des pictogrammes « plein écran » et « recadrer », la poignée pleine fait la différence. L'outil
**Recadrer** de l'éditeur utilise l'icône de recadrage du système (Adwaita symbolic, SF Symbols), jamais le symbole
de Kropa, qui reste réservé à l'application (icône, barre système, logo).

## 8. Nom

Recherche d'antériorité et finalistes : [`naming.md`](naming.md). **Ce n'est pas une vérification juridique** : avant
toute publication, consulter les bases de marques (EUIPO, INPI, USPTO, WIPO) en classes 9 et 42 et vérifier les
domaines. À réserver : crate `kropa`, dépôt GitHub, identifiant d'application (`io.github.<compte>.Kropa`), nom sur
Flathub et Homebrew. Logo : ne reprend aucun logo connu de capture d'écran (Flameshot, ShareX, Spectacle,
CleanShot X, Snagit, Greenshot, Lightshot) ; les équerres rappellent le pictogramme générique « recadrer », sans
appartenir à une marque.

## 9. Identité B archivée

L'identité B « Boucle » (boucle corail tracée à la main, papier crème, Bricolage Grotesque + Atkinson Hyperlegible)
n'a pas été retenue : son corail entrait en conflit avec les états d'erreur et son trait « à main levée » coûtait à
chaque nouvelle icône. Proposition complète, SVG et comparaison avec A : [`archive/b/`](archive/b/README.md). Ne pas
l'utiliser pour le produit.

## 10. Fichiers

```
docs/brand/
  brand.md            ce document
  naming.md           recherche du nom
  preview.html        aperçu autonome de l'identité A (SVG embarqués, Google Fonts seulement)
  a/ logo.svg logo-inverse.svg logo-mono.svg icon.svg icon-linux.svg icon-linux-light.svg icon-symbolic.svg
  archive/b/          identité B archivée (README.md et SVG de la proposition)
```
Notes techniques : les logos gardent des traits SVG (rendu exact, fichiers légers) ; les vectoriser avant
impression. L'icône macOS se convertit en `.icns` depuis `icon.svg` (1024) au moment du build ; sous Linux,
installer la variante Linux retenue (`icon-linux.svg` ou `icon-linux-light.svg`) dans `hicolor/scalable/apps/<app-id>.svg` et `icon-symbolic.svg` dans
`hicolor/symbolic/apps/<app-id>-symbolic.svg`.
