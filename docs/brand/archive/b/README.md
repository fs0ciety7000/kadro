# Identité B « Boucle » — archivée

> Archivée le 11/11/2026 : l'utilisateur a retenu l'identité A « Repère » et le nom **kropa**. Ce document conserve
> la proposition B telle qu'elle a été présentée (sous l'ancien nom de travail « Kadro », d'où le mot-symbole et les
> titres des SVG de ce dossier). Ne pas utiliser pour le produit. Identité en vigueur : [`../../brand.md`](../../brand.md).

Nuance de ton propre à B : une demi-phrase plus explicative que A, par exemple pour l'historique vide :
« Vos captures apparaîtront ici, rangées par date. Pour commencer : Impr. écran, ou `kadro capture area`. »

## Proposition

**Concept.** Le cadre qu'on trace à la main autour de ce qui compte : le rectangle rouge que tout le monde dessine
sur une capture pour un ticket de support ou une page de documentation. Un seul geste, légèrement penché, dont les
deux bouts **se croisent** au coin supérieur gauche. Le mot « kadro » est lui-même entouré : la marque s'annote.

**Logo** (`logo.svg`, `logo-inverse.svg`, `logo-mono.svg`). Mot-symbole bas de casse arrondi, terminaisons
rondes, entouré de la boucle corail inclinée de −2°.
- Zone de protection : trois épaisseurs de boucle autour de la boucle (les bouts qui dépassent comptent dans le logo).
- Taille minimale : 72 px de large ; en dessous, la boucle seule (symbole).
- Une couleur : boucle et mot dans la même couleur (`logo-mono.svg`).

**Icônes.**
- `icon.svg` : macOS, plaque papier crème, trois lignes de contenu, boucle corail penchée de −4° qui reste à
  l'intérieur de la plaque.
- `icon-linux.svg` : freedesktop, **sans plaque** : une miniature de capture (carte paysage avec épaisseur) que la
  boucle déborde en haut à gauche et en bas ; silhouette irrégulière, unique dans une grille d'icônes Adwaita.
- `icon-symbolic.svg` : 16 × 16, anneau à coins arrondis avec le coin croisé, aplats seulement, **sans
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

## Comparaison avec A au moment du choix

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
