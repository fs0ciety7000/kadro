# Kadro — identité de marque (propositions A et B)

> Équipe Direction artistique. Deux identités complètes, une recommandation. Rien n'est figé avant le choix de
> l'utilisateur. Aperçu visuel : [`preview.html`](preview.html) (à ouvrir dans un navigateur).

## 1. Positionnement

**Promesse.** Kadro capture, annote et partage votre écran sans quitter le clavier, et vos images restent chez vous
jusqu'au serveur que vous avez choisi.

- **Personnalité** : précis (au pixel, au raccourci près) ; sobre (aucun effet gratuit, aucune fanfare) ; loyal (dit
  ce qu'il fait, ce qu'il envoie, et ce qui a échoué).
- **Kadro n'est pas** un service en ligne, un réseau social de captures, un outil « magique » qui cache ce qu'il fait,
  ni une usine à options exposées d'emblée.
- **Le nom** : *kadr* (cadre, prise de vue en polonais et en russe), *kadro* (cadre en espéranto). Les deux identités
  partent du cadre, par deux chemins opposés.

## 2. Ton de voix (commun aux deux identités)

Règles : phrase courte, sujet clair ; on dit **ce qui s'est passé, où est le fichier, quoi faire** ; pas de « Oups »,
pas de point d'exclamation, pas d'humour dans les erreurs ; on montre la commande CLI équivalente quand elle aide ;
vouvoiement en français ; mêmes termes partout (capture, envoi, destination, historique).
A et B ne diffèrent que par la nuance : A plus télégraphique, B une demi-phrase plus explicative.

| Situation | FR | EN |
|---|---|---|
| Notification « lien copié » | **Lien copié** · `https://img.maison.lan/u/a3F9k.png` — Enregistrée dans ~/Images/Kadro. [Ouvrir] [Annoter] | **Link copied** · `https://img.maison.lan/u/a3F9k.png` — Saved to ~/Pictures/Kadro. [Open] [Annotate] |
| Erreur d'envoi | **Envoi échoué** : Zipline a refusé le jeton (401). La capture est enregistrée en local et reste dans la file d'attente. [Réessayer] [Voir le journal] | **Upload failed**: Zipline rejected the token (401). The capture is saved locally and stays in the queue. [Retry] [View log] |
| Erreur réseau (variante) | **Envoi en attente** : `img.maison.lan` ne répond pas. Nouvel essai dans 30 s. | **Upload pending**: `img.maison.lan` is not responding. Retrying in 30 s. |
| Historique vide | Aucune capture pour l'instant. Appuyez sur Impr. écran ou lancez `kadro capture area`. | No captures yet. Press Print Screen or run `kadro capture area`. |
| Historique vide (B) | Vos captures apparaîtront ici, rangées par date. Pour commencer : Impr. écran, ou `kadro capture area`. | Your captures will show up here, sorted by date. To start: Print Screen, or `kadro capture area`. |

Dans l'application, le texte d'interface utilise **la police du système** (Adwaita Sans / Cantarell sous GNOME,
SF Pro sous macOS), par cohérence avec le principe « natif ». Les polices de marque servent au site, à la
documentation, au README et aux visuels.

---

## 3. Identité A — « Repère »

**Concept.** La zone éclairée : quand on sélectionne une zone, l'écran s'assombrit et seule la sélection reste
claire. Le logo garde deux équerres opposées (le cadre, le nom) et la **poignée** ambre qu'on tire au coin
inférieur droit (le geste). Pas d'appareil photo, pas d'objectif, pas de viseur centré.

**Logo** (`a/logo.svg`, `a/logo-inverse.svg`, `a/logo-mono.svg`). Symbole + mot-symbole « kadro » en bas de casse
monolinéaire géométrique, terminaisons coupées net, dessiné sur la même grille que les équerres.
- Zone de protection : la largeur de la poignée tout autour (≈ 1/3 de la hauteur du symbole).
- Taille minimale : logo complet 80 px de large ; en dessous, le symbole seul (dessiné pour 16 px).
- Une couleur : la poignée passe à la couleur du trait (`logo-mono.svg`) et reste lisible grâce à son épaisseur.

**Icônes.**
- `a/icon.svg` : macOS, gabarit 1024 (plaque superellipse 824 px, ombre incluse). Plaque graphite, zone éclairée,
  équerres blanches, poignée ambre.
- `a/icon-linux.svg` : freedesktop/GNOME, grille 128, **sans plaque imposée** : la silhouette est un écran-carte
  paysage avec épaisseur en bas (convention Adwaita), bord clair à 14 % pour se détacher sur fond sombre.
- `a/icon-symbolic.svg` : 16 × 16, aplats seulement (pas de trait, pour que GTK recolore), `#2e3436` ; alignés au
  pixel. Sur macOS, l'exporter en image *template* (PDF ou PNG noir + alpha, nom en `…Template`).

**Palette** (contrastes WCAG 2.x mesurés ; texte normal : 4,5 requis, composants graphiques : 3 requis).

#### A — clair

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

#### A — sombre

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


Règles A : l'ambre ne porte jamais seul une information en mode clair (2,20:1 sur le fond) ; il sert de
**remplissage** avec du texte `#111318` dessus (7,88:1). Pour un trait ou un texte d'accent en clair : `accent-text`.

**Typographies** (licence libre, OFL 1.1).
- Titres et site : **Space Grotesk** — [github.com/floriankarsten/space-grotesk](https://github.com/floriankarsten/space-grotesk), [Google Fonts](https://fonts.google.com/specimen/Space+Grotesk).
- Code, CLI, raccourcis, URL : **JetBrains Mono** — [github.com/JetBrains/JetBrainsMono](https://github.com/JetBrains/JetBrainsMono), [Google Fonts](https://fonts.google.com/specimen/JetBrains+Mono). Distingue nettement 0/O, 1/l/I.

**Principes d'icône (barre d'outils, interface).** Grille 20 px, trait 1,75 px, extrémités carrées et angles vifs,
formes géométriques simples ; un seul accent (ambre) réservé à l'outil actif ; **lettre du raccourci visible sous
chaque outil**. Pour les actions standard (annuler, copier…), utiliser les icônes du système (Adwaita symbolic,
SF Symbols) ; les icônes maison ne couvrent que les outils propres à Kadro.

---

## 4. Identité B — « Boucle »

**Concept.** Le cadre qu'on trace à la main autour de ce qui compte : le rectangle rouge que tout le monde dessine
sur une capture pour un ticket de support ou une page de documentation. Un seul geste, légèrement penché, dont les
deux bouts **se croisent** au coin supérieur gauche. Le mot « kadro » est lui-même entouré : la marque s'annote.

**Logo** (`b/logo.svg`, `b/logo-inverse.svg`, `b/logo-mono.svg`). Mot-symbole bas de casse arrondi, terminaisons
rondes, entouré de la boucle corail inclinée de −2°.
- Zone de protection : trois épaisseurs de boucle autour de la boucle (les bouts qui dépassent comptent dans le logo).
- Taille minimale : 72 px de large ; en dessous, la boucle seule (symbole).
- Une couleur : boucle et mot dans la même couleur (`logo-mono.svg`).

**Icônes.**
- `b/icon.svg` : macOS, plaque papier crème, trois lignes de contenu, boucle corail penchée de −4° qui reste à
  l'intérieur de la plaque.
- `b/icon-linux.svg` : freedesktop, **sans plaque** : une miniature de capture (carte paysage avec épaisseur) que la
  boucle déborde en haut à gauche et en bas ; silhouette irrégulière, unique dans une grille d'icônes Adwaita.
- `b/icon-symbolic.svg` : 16 × 16, anneau à coins arrondis avec le coin croisé, aplats seulement, **sans
  inclinaison** (à 16 px, la pente rendrait les bords flous).

**Palette.**
#### B — clair

| Jeton | Hex | Rôle | Contraste mesuré |
|---|---|---|---|
| `paper` | `#FBF7F0` | Fond de fenêtre (papier) | — |
| `surface` | `#FFFFFF` | Panneaux, barre d'outils, miniature | — |
| `line` | `#E6DFD3` | Séparateurs (décoratif) | 1.32:1 sur surface (décoratif) |
| `line-strong` | `#8A8075` | Contours de champs, outils inactifs | 3.62:1 sur paper (OK graphique (≥ 3)); 3.87:1 sur surface (OK graphique (≥ 3)) |
| `ink` | `#1F1B16` | Texte principal, bouton principal (texte papier dessus) | 16.03:1 sur paper (AAA); 17.12:1 sur surface (AAA) |
| `ink-muted` | `#6B6257` | Texte secondaire, raccourcis | 5.60:1 sur paper (AA); 5.98:1 sur surface (AA) |
| `coral` | `#EF4F3A` | Corail : la boucle, couleur d'annotation par défaut ; graphique seulement | 3.36:1 sur paper (OK graphique (≥ 3)); 3.59:1 sur surface (OK graphique (≥ 3)) |
| `coral-strong` | `#C93A22` | Texte d'accent, liens, focus | 4.78:1 sur paper (AA); 5.11:1 sur texte blanc posé dessus (AA) |
| `highlight` | `#FFD43B` | Surligneur : fond de mise en avant, texte encre dessus | 12.01:1 sur texte ink posé dessus (AAA) |
| `success` | `#2F7D4A` | Succès (« Lien copié ») | 4.73:1 sur paper (AA) |
| `danger` | `#B42318` | Erreur (toujours avec icône et mot) | 6.16:1 sur paper (AA) |

#### B — sombre

| Jeton | Hex | Rôle | Contraste mesuré |
|---|---|---|---|
| `paper` | `#1C1915` | Fond de fenêtre | — |
| `surface` | `#26221D` | Panneaux, barre d'outils, miniature | — |
| `raised` | `#312C26` | Survol, éléments en relief | — |
| `line` | `#433C34` | Séparateurs (décoratif) | 1.46:1 sur surface (décoratif) |
| `line-strong` | `#8C8174` | Contours de champs, outils inactifs | 4.14:1 sur surface (OK graphique (≥ 3)) |
| `ink` | `#F3EEE6` | Texte principal, bouton principal (texte encre dessus) | 13.68:1 sur surface (AAA); 11.97:1 sur raised (AAA) |
| `ink-muted` | `#B3A99B` | Texte secondaire, raccourcis | 6.82:1 sur surface (AA); 5.97:1 sur raised (AA) |
| `coral` | `#FF7A63` | Corail : boucle, annotation, texte d'accent, focus | 6.18:1 sur surface (AA); 5.41:1 sur raised (AA) |
| `highlight` | `#FFD43B` | Surligneur (texte encre #1F1B16 dessus) | 12.01:1 sur texte #1F1B16 posé dessus (AAA) |
| `success` | `#6FCF8E` | Succès | 8.27:1 sur surface (AAA) |
| `danger` | `#FF8A80` | Erreur (toujours avec icône et mot) | 6.92:1 sur surface (AA) |
Règles B : le corail est la couleur de la marque **et** la couleur d'annotation par défaut ; il n'est **jamais** un
bouton (un bouton rouge se lit comme « supprimer »). Le bouton principal est à l'encre. Les erreurs, proches du
corail, s'affichent toujours avec une icône et un mot (« Envoi échoué »), jamais par la seule couleur.

**Typographies** (OFL 1.1).
- Titres : **Bricolage Grotesque** — [github.com/ateliertriay/bricolage](https://github.com/ateliertriay/bricolage), [Google Fonts](https://fonts.google.com/specimen/Bricolage+Grotesque).
- Texte, documentation : **Atkinson Hyperlegible** (Braille Institute) — [github.com/googlefonts/atkinson-hyperlegible](https://github.com/googlefonts/atkinson-hyperlegible), [Google Fonts](https://fonts.google.com/specimen/Atkinson+Hyperlegible). Dessinée pour la lisibilité (malvoyance) : cohérent avec un public support et documentation. Complément pour le code : Atkinson Hyperlegible Mono (OFL).

**Principes d'icône.** Grille 20 px, trait 2 px, extrémités et angles arrondis, formes un peu souples ; l'outil actif
est **entouré d'une mini-boucle** (signal de forme, pas seulement de couleur) ; raccourci affiché dans l'infobulle.
Mêmes règles d'emprunt aux icônes système que A.

---

## 5. Comparaison et recommandation

| Critère | A — Repère | B — Boucle |
|---|---|---|
| Lien au produit | Le geste de capture (sélection + poignée) | Le geste d'annotation (le cadre tracé) |
| Public le mieux servi | Développeurs, admins, auto-hébergement | Rédaction de documentation, support |
| Principes « clavier, scriptable » | Exprimés (raccourcis visibles, mono, ton télégraphique) | Présents mais en retrait |
| Lisibilité à 16 px | Très bonne : aplats au pixel | Bonne : l'anneau reste net, la pente est sacrifiée |
| Distinction parmi Adwaita / macOS | Bonne : plaque graphite et point ambre rares ; risque d'être vue comme une icône de terminal à distance | Très bonne : crème, corail, forme irrégulière |
| Risque de confusion | Symbolique proche des icônes « plein écran » et « recadrer » ; la poignée pleine fait la différence | Symbolique proche d'une icône « recadrer » ; corail proche du rouge d'erreur |
| Tenue dans la durée | Système géométrique facile à étendre (site, icônes d'outils) | Style « fait main » exigeant : chaque nouvelle icône doit garder le même geste |

**Recommandation : A — Repère.** Elle traduit le principe premier du produit (le geste de capture, au pixel, au
clavier) et parle d'abord à la cible principale du brief (développeurs, admins, auto-hébergement). Son système
géométrique est le plus simple à tenir pour un développeur seul : icônes d'outils, site, captures de la doc
se déclinent sans talent d'illustrateur. Elle est saine en accessibilité (accent de remplissage à 7,88:1, aucune
couleur d'état confondue avec la marque). B est plus mémorable et plus chaleureuse, mais son corail entre en
conflit avec les états d'erreur et son trait « à main levée » coûte à chaque nouvelle icône.
Points à surveiller si A est retenue : vérifier en situation (dock GNOME, Launchpad) qu'on ne la confond pas avec
une icône de terminal ; garder la poignée pleine dans toutes les déclinaisons.

---

## 6. Recherche d'antériorité du nom « Kadro » (11/11/2026)

> Recherche rapide (web, GitHub, registres de paquets). **Ce n'est pas une vérification juridique** : avant toute
> publication, consulter les bases de marques (EUIPO, INPI, USPTO, WIPO) dans les classes 9 et 42.

- **Gênant — même famille de produits** : un **éditeur vidéo libre pour Windows nommé « Kadro »** existe sur GitHub
  (`kadro-studio/kadro`, `honeyjostar/kadro`) et une fiche « Kadro Video Editor » figure sur AlternativeTo.
  Logiciel de création visuelle, libre, desktop : risque réel de confusion dans les recherches et les dépôts de
  paquets (Flathub, Homebrew).
- **Paquet npm `kadro`** pris (cadriciel d'agents auto-hébergé, version de réservation, juillet 2026). Pas de conflit
  de catégorie, mais le nom est occupé sur npm.
- **Entreprises homonymes** : Kadro, studio numérique (kadro.app) ; Kadro, agence e-commerce (Raleigh, États-Unis).
  Noms proches : Kadromierz (planning RH, Pologne), CADRO HRMS, Kardo. Projets GitHub mineurs : `koaning/kadro`
  (bibliothèque Python), `witek/kadro` (navigateur sans cadre).
- **Libre** au moment de la recherche : crate `kadro` sur crates.io, PyPI, aucune application « Kadro » sur Flathub.
- **Sens dans d'autres langues** : « kadro » signifie « effectif, équipe » en turc ; neutre.
- **Logo** : aucune des deux identités ne reprend un logo connu de capture d'écran (Flameshot, ShareX, Spectacle,
  CleanShot X, Snagit, Greenshot, Lightshot). Les équerres de A rappellent le pictogramme générique « recadrer »,
  sans appartenir à une marque.

**Conseil** : réserver dès maintenant le crate `kadro`, un identifiant d'application (par ex. `io.github.<compte>.Kadro`)
et le nom sur Flathub/Homebrew ; envisager un nom de dépôt et de paquet explicite (`kadro-capture`) pour se
distinguer de l'éditeur vidéo homonyme.

Sources : [éditeur vidéo Kadro, AlternativeTo](https://alternativeto.net/software/kadro-video-editor/) ·
[Kadro sur CB Insights](https://www.cbinsights.com/compare/kadro-vs-softslate) ·
[Kadromierz](https://sourceforge.net/software/product/Kadromierz) ·
[CADRO HRMS](https://www.capterra.co.uk/software/1099114/CADRO-HRMS) ·
[npm `kadro`](https://www.npmjs.com/package/kadro) · [crates.io](https://crates.io/crates/kadro) ·
[métadonnées Google Fonts (licences)](https://github.com/google/fonts/tree/main/ofl).

## 7. Fichiers

```
docs/brand/
  brand.md            ce document
  preview.html        page de comparaison autonome (SVG embarqués, Google Fonts seulement)
  a/ logo.svg logo-inverse.svg logo-mono.svg icon.svg icon-linux.svg icon-symbolic.svg
  b/ logo.svg logo-inverse.svg logo-mono.svg icon.svg icon-linux.svg icon-symbolic.svg
```
Notes techniques : les logos gardent des traits SVG (rendu exact, fichiers légers) ; les vectoriser avant
impression. L'icône macOS se convertit en `.icns` depuis `icon.svg` (1024) au moment du build ; sous Linux,
installer `icon-linux.svg` dans `hicolor/scalable/apps/<app-id>.svg` et `icon-symbolic.svg` dans
`hicolor/symbolic/apps/<app-id>-symbolic.svg`.
