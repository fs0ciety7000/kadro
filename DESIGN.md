# DESIGN.md — Kropa

> Direction visuelle et règles d'interface, à respecter par toute interface (lots 2.3, 2.4, 3.5, 3.6, 4.1, 4.2, 4.10,
> 4.11). État : **proposition D2.1 / D2.2 révisée après la critique D2.3 du 11/11/2026**
> (`docs/reviews/2026-11-11-design-D2.3.md`), à valider par l'utilisateur. Sources : `docs/decisions.md` (prioritaire),
> `docs/brand/brand.md` (identité A « Repère », validée), `docs/product.md`, `docs/cli.md`, `docs/platform-linux.md`,
> `docs/platform-macos.md`, `docs/security/secrets.md`.
> Maquettes : `docs/design/mockups.html` (9 écrans + icônes ; libadwaita / AppKit, clair / sombre, FR / EN, états),
> sans aucune ressource externe. Icônes d'outils : `docs/design/icons/`.

## 0. Changements après critique D2.3

| Constat de la critique | Correction |
|---|---|
| **B1** La classe `.sel` de l'overlay (voile de 3000 px) touchait la rangée active de la liste d'annotations : libellés superposés, 5 rangées pour 6 objets, voile sur l'éditeur | Classe renommée `.ov-sel` ; la rangée active est désignée par `aria-selected="true"`. Liste rendue en 6 rangées, sans voile |
| **B2** Texte < 4,5:1 : « Indisponible sur Sway » sous `opacity:.55`, « Par défaut » blanc sur `#3584E4` (3,77:1) | Seul le titre d'une rangée inactive est grisé (composant inactif, exempté par WCAG 1.4.3) ; la raison reste en `k-muted` plein + icône info. Étiquette « Par défaut » sur fond `--btn` neutre, texte `--fg`, avec coche |
| I1, I2 Pas de grand déplacement, modificateurs incohérents entre overlay et éditeur | Règle unique partout : flèches 1 px, **Maj = ×10**, **Alt / ⌥ = redimensionner** (Alt+Maj ×10). Pas de pas de 100 px : les grands déplacements passent par Ctrl+A / ⌘A puis réduction (§ 8.1) |
| I3 `Entrée` sans sélection = écran entier | **Ctrl+A / ⌘A** sélectionne l'écran actif, ajustable ; `Entrée` sans zone ne fait rien et rappelle l'indice |
| I4 `Tab` écran suivant | Validé ; masqué s'il n'y a qu'un écran ; annonce lecteur d'écran ; message de bord « La zone ne peut pas dépasser cet écran » |
| I5 Aide de l'overlay | Actions principales en première ligne, en gras ; règle de priorité et de retournement des HUD ; repli sur `?` si l'aide couvre la zone ; libellé de `Entrée` dynamique (absent sans zone) |
| I6 Sélection d'objet, Recadrer, Échap, F6 | Clic sur un objet = sélection quel que soit l'outil + outil **V** ; **Recadrer (C) proposé**, icône système (§ 8.2) ; Échap ne ferme jamais (Ctrl+W / ⌘W) ; F6 documenté |
| I7 Fin d'envoi sous GNOME | Fenêtre au premier plan : copie directe + `AdwToast` « Lien copié » ; « Lien prêt » seulement sans focus ; l'éditeur reste ouvert |
| I8 Surligneur en `multiply` invisible sur fond sombre | Jaune à 45 % en mélange normal |
| I9 Cibles | Zone de prise ≥ 24 px partout (poignées de 9 px comprises), règle des petites zones ; 44 px barre d'outils et fermeture de la miniature ; pastilles 36 px |
| I10 Miniature | Raccourcis visibles seulement au focus ; réglage « Jusqu'à fermeture » ; pas de fermeture automatique avec VoiceOver ; annonce ; décompte en `k-accent-text` ; fermeture 44 px |
| I11 Pas d'écran « menu résident » | Nouvel écran (i) : menu de la barre macOS, barre de menus de l'app, zone de notification KDE, GNOME sans icône (lanceur, actions `.desktop`) |
| I12 Refus GNOME renvoyé vers « Paramètres › Applications » (non vérifié) | Procédure de `platform-linux.md` § 4 : effacer le refus du permission store (`flatpak permission-reset`, ou commande donnée par `kropa doctor`), puis Réessayer |
| I13 Focus visible | Anneau libadwaita documenté comme écart système ; outils : 2 px `k-text` décalés de 2 px ; état « focus sur outil actif » dessiné |
| I14 Pastilles sans contour | Contour `k-line-strong` (≥ 3:1) sur chaque pastille |
| I15 Noms accessibles, vocabulaire | Nom complet (type, couleur, taille, position) ; « Compteur », « Couleur », « bandeau » ; un seul numéro visible (celui du compteur) |
| I16 Import `.sxcu` | Forme punycode affichée à côté de l'Unicode ; bouton qui passe à la ligne |
| Point 1 des réponses : AccentColor macOS | Exception écrite au § 3 ; **à reporter dans `brand.md`** (non modifié ici) |
| M1-M21 | Voir § 15 (traités) et § 14 (restants) |
| Point 8 : états manquants | Ajoutés : création du coffre, saisie de texte, focus visible, fenêtre minimale 720 × 480, échec du test, import `http` refusé, Sway sans portail, menu résident, outil de sélection, barre de menus macOS, fenêtre « Raccourcis clavier » |

## 1. Cadre

- **Public** : Léa (développeuse, GNOME Wayland), Marc (admin, KDE), puis Sophie et Karim (macOS, Ubuntu).
- **Tâche principale** : zone → annotation → lien, au clavier, en moins de trois touches hors annotation.
- **Plateformes** : GTK 4 + libadwaita (GNOME HIG) sous Linux ; AppKit (Apple HIG) sous macOS 15+.
- **Contraintes** : clair et sombre, FR et EN, tout au clavier, contraste texte ≥ 4,5:1, jamais de succès simulé,
  aucune requête réseau non configurée.

## 2. Principes

1. **Natif d'abord.** Fenêtres, contrôles, dialogues, notifications, menus et polices sont ceux du système. On ne
   dessine pas de thème Kropa par-dessus libadwaita ni AppKit.
2. **L'identité vit là où l'écran nous appartient** : l'overlay de sélection, l'outil actif, les icônes d'outils,
   la miniature, l'icône d'application et l'icône de barre système. C'est là que sert l'ambre.
3. **Précis.** Pixels physiques affichés, loupe au pixel, poignées à angles vifs, chiffres tabulaires.
4. **Loyal.** Chaque état dit ce qui s'est passé, où est le fichier, quoi faire ; une limite de plateforme se dit
   (GNOME, wlroots, macOS), elle ne se cache pas.
5. **Sobre.** Une action principale par écran, pas d'animation gratuite, densité de bureau.

## 3. Partage du territoire

| Élément | Qui le dessine | Ce que Kropa y met |
|---|---|---|
| Overlay de sélection, loupe, aide clavier | Kropa (surface plein écran ou layer-shell, `NSPanel`) | tout : voile, sélection, poignée ambre, HUD |
| Barre d'outils de l'éditeur | widgets natifs (`GtkToggleButton`, `NSButton`) | icônes maison, lettre visible, état actif ambre |
| Canevas et annotations | Kropa (`kropa-render`) | palette d'annotation, compteurs, masques |
| Miniature (macOS, KDE, wlroots) | conteneur natif, contenu Kropa | aperçu, actions, décompte |
| Menu résident (barre macOS, zone de notification KDE) | système (`NSMenu`, menu StatusNotifierItem dessiné par Plasma) | symbole Kropa, entrées et libellés |
| Fenêtres (réglages, file, coffre, premier lancement, lanceur GNOME) | 100 % natif | textes, ordre, états ; aucune couleur de marque |
| Notifications | système (portail Notification, `UNUserNotificationCenter`) | textes et actions seulement |

**Exception macOS (seule couleur de marque dans une fenêtre native).** La ressource `AccentColor` de l'app vaut
**ambre profond** `#A84B00` (clair) / `#B85400` (sombre). macOS ne l'applique qu'en accent « Multicolore » (réglage
par défaut) ; sinon le choix de l'utilisateur l'emporte. Elle colore le bouton par défaut, les interrupteurs, la
sélection des listes et des menus. Texte blanc dessus : 5,72:1 / 4,88:1. L'ambre vif `#FF8A1F` y est exclu (le
système y pose du texte blanc : 2,36:1). Sous GNOME, aucune exception : l'accent reste celui de l'utilisateur.
**À reporter dans `docs/brand/brand.md` § 6** (règle d'usage de l'ambre), non modifié ici.

## 4. Couleurs

### 4.1 Jetons Kropa (surfaces à nous)

Repris de `brand.md` (valeurs inchangées) ; jetons ajoutés marqués « + ». Contrastes WCAG 2.x (luminance sRGB).

| Jeton | Clair | Sombre | Rôle | Contraste mesuré |
|---|---|---|---|---|
| `k-surface` | `#FFFFFF` | `#1A1D24` | fond du HUD (aide, dimensions, loupe), miniature KDE | — |
| `k-raised` | `#F0F1F4` | `#262A33` | touches (`kbd`) dans le HUD | — |
| `k-line` | `#D9DCE2` | `#3A3F4A` | séparateurs, grille de la loupe (décoratif) | — |
| `k-line-strong` | `#7D8490` | `#7A818D` | contour de la loupe, bas de touche, **contour des pastilles de couleur** | 3,77:1 / 4,30:1 (graphique ≥ 3) |
| `k-text` | `#15171C` | `#ECEEF2` | texte du HUD, **anneau de focus des outils** | 17,93:1 / 14,52:1 sur surface |
| `k-muted` | `#5A606B` | `#A2A8B3` | libellés secondaires du HUD **et** texte secondaire informatif des fenêtres (§ 4.3) | 6,32:1 / 7,06:1 sur surface |
| `k-accent` | `#FF8A1F` | `#FF8A1F` | remplissage : outil actif, poignée | 7,88:1 avec `#111318` posé dessus ; 2,36:1 sur blanc (jamais seul) |
| `k-on-accent` | `#111318` | `#111318` | icône et lettre sur l'ambre | 7,88:1 |
| `k-accent-text` | `#A84B00` | `#FF8A1F` | liseré de l'outil actif en clair, anneau de focus de l'overlay, **décompte de la miniature** | 5,72:1 sur blanc / 7,15:1 sur `#1A1D24` |
| + `k-veil` | `rgba(17,19,24,.55)` | idem | voile hors sélection (constant : il couvre l'écran de l'utilisateur) | sans texte |
| + `k-handle` | `#FFFFFF`, bord et liseré `#111318` (`rgba(17,19,24,.65)` pour le bord de zone) | idem | bord de sélection, poignées de sélection et d'objet (fusion de `k-sel-edge` et `k-handle`, M) | blanc/`#111318` 18,58:1 → au moins 4,31:1 contre tout fond |
| `k-success` / `k-danger` | `#1A7F45` / `#C62828` | `#4ADE80` / `#FF6B6B` | icônes d'état de nos surfaces | 5,04:1 / 5,62:1 sur blanc |

Aucune valeur codée en dur dans le code d'interface : le liseré de l'outil actif est `k-accent-text`, pas `#A84B00`.
Contrôles complémentaires : liseré contre la barre blanche 5,72:1 ; ambre contre la barre sombre GNOME `#2E2E32`
5,73:1 et la fenêtre sombre macOS `#2C2C2E` 5,91:1 ; `k-text` sur `k-raised` 15,88:1 / 12,37:1. Un test CI calcule
ces contrastes à partir des jetons (lot 2.3).

### 4.2 Accent et couleurs système (fenêtres)

- **GNOME** : on **n'écrase pas** l'accent système (choix de l'utilisateur depuis GNOME 47). Boutons `suggested-action`,
  interrupteurs, sélection, liens : couleurs libadwaita (`@accent_bg_color`, `@accent_color`…).
- **macOS** : `AccentColor` ambre profond, exception décrite au § 3.
- **Écart système accepté** : le bouton principal par défaut reste sous 4,5:1 quand l'utilisateur garde le bleu
  (GNOME `#3584E4` + blanc gras 3,77:1 ; macOS `#007AFF` + blanc 4,02:1). Rendu système, non modifié par Kropa.
  **Aucun composant Kropa ne reprend cette combinaison** (B2) : l'étiquette « Par défaut » est neutre.
- **Couleur d'état** : la couleur ne porte jamais seule un état. Le texte d'état garde la couleur de texte
  principale ; l'icône porte la couleur (`success`, `error`, `warning` du système). Exception : message d'erreur
  sous un champ (GNOME `@error_color` `#C30000` 6,06:1 sur `#FAFAFB`, sombre `#FF938C` 7,40:1 sur `#222226` ;
  macOS : texte principal + icône rouge, car `systemRed` fait 3,6:1 sur blanc).
- **Notifications** : aucun bouton d'accent ; le système les dessine (M).

### 4.3 Texte secondaire et état inactif

`.dim-label` de libadwaita (opacité 0,55, ≈ 3,2:1 sur blanc) et `secondaryLabelColor` de macOS (≈ 4:1) sont
**réservés au texte non essentiel**. Une information (chemin, hôte, état, raccourci, **raison d'une indisponibilité**)
utilise `k-muted` : `#5A606B` = 6,06:1 sur `#FAFAFB` (GNOME) et 5,35:1 sur `#ECECEC` (macOS) ; `#A2A8B3` = 6,63:1 sur
`#222226` (GNOME) et 5,83:1 sur `#2C2C2E` (macOS). Taille ≥ 12 px. Mise en œuvre : classe `.kropa-muted` (fournisseur
de style applicatif) sous GTK, `NSColor` dynamique sous AppKit, avec variante « Augmenter le contraste » ; en mode
contraste élevé, on rend la main au système.

**Rangée inactive** (B2) : seul le titre prend l'opacité 0,55 (composant inactif, exempté par WCAG 1.4.3) ; la raison
est écrite en `k-muted` plein, précédée d'une icône info, et dit quoi faire (« Indisponible sur Sway : utilisez la
ligne ci-dessus »). Jamais d'opacité sur une rangée entière.

### 4.4 Palette d'annotation (dessinée dans l'image)

| Nom | Hex | Contraste vs blanc / noir | Chiffre des compteurs |
|---|---|---|---|
| Rouge (défaut) | `#E01B24` | 4,83 / 4,35 | `#FFFFFF` (4,83:1) |
| Ambre | `#FF8A1F` | 2,36 / 8,91 | `#111318` (7,88:1) |
| Jaune | `#F6D32D` | 1,47 / 14,28 | `#111318` (12,63:1) |
| Vert | `#2EC27E` | 2,30 / 9,13 | `#111318` (8,08:1) |
| Bleu | `#1C71D8` | 4,77 / 4,40 | `#FFFFFF` (4,77:1) |
| Noir | `#15171C` | 17,93 / — | `#FFFFFF` (17,93:1) |
| Blanc | `#FFFFFF` | — / 21 | `#111318` (18,58:1) |

- **Contour de lisibilité** (activé par défaut) : liseré de 1,5 px (× échelle) blanc pour les couleurs sombres,
  `#15171C` pour les claires (luminance > 0,4) ; il garantit ≥ 3:1 contre le fond capturé.
- **Pastilles du sélecteur** : contour `k-line-strong` 1 px autour de chaque pastille (le blanc et le jaune restent
  visibles sur fond clair, le noir sur fond sombre) ; la pastille choisie porte une coche et un anneau `--fg` ; nom
  accessible « Rouge », état « coché ». Jamais la couleur seule.

## 5. Typographie

Interface : **police système uniquement**. Pas de police de marque dans l'application.

| Rôle | libadwaita (Adwaita Sans ; Cantarell avant GNOME 48) | AppKit (SF Pro) |
|---|---|---|
| Titre d'écran vide, premier lancement | `.title-1` 20 pt (26,7 px) 800 | 22 px 700 |
| Titre de dialogue | `.title-2` 15 pt (20 px) 800 | 13 px 700 (`NSAlert`) |
| Titre de groupe, en-tête | `.heading` 11 pt (14,7 px) 700 | 13 px 700 |
| Texte courant | 11 pt (14,7 px) 400 | 13 px 400 |
| Sous-titre de rangée, légende | 9–10 pt (12–13 px) | 11 px |
| Lettre d'outil | 11 px 700 | 11 px 700 |
| Code, URL, chemins, hôtes | Adwaita Mono, `.monospace` | SF Mono |

- Chiffres tabulaires (`tnum`) pour dimensions, coordonnées, pourcentages, minuteries.
- **HUD de l'overlay** : il suit l'échelle du texte du système (GNOME `text-scaling-factor` ; macOS : pas de réglage
  global, taille fixe) ; les mises en page de l'aide passent à la ligne au lieu de déborder (M).
- **Texte des annotations** (dessiné dans l'image) : **Inter** (OFL 1.1), embarquée en sous-ensemble statique avec sa
  licence, pour un rendu identique sur les deux OS et des instantanés stables (lot 3.3). Couverture : latin, cyrillique,
  grec ; **hors de ces écritures, repli sur la police système** (Noto sous Linux, police système sous macOS), et
  l'instantané de test ne couvre que les écritures embarquées.
- **Maquettes** : aucune police téléchargée (M19). Les familles nommées (Adwaita Sans, Cantarell, Inter, SF Pro,
  JetBrains Mono) ne servent que si elles sont installées ; sinon `system-ui`. Le rendu exact dépend de la machine.

## 6. Espacements, dimensions, rayons, ombres

**Grille** : 4 px sur nos surfaces (`4, 8, 12, 16, 24, 32`). Dans les fenêtres, marges natives : multiples de 6 sous
libadwaita (6, 12, 18, 24 ; largeur de contenu 600 px, `AdwClamp`), 20 px de marge de fenêtre et 8 px entre
contrôles sous AppKit.

| Élément | Valeur |
|---|---|
| Outil de la barre | 44 × 52 px, icône 20 px, lettre 11 px dessous, 4 px d'écart entre outils (2 px sous 760 px de large), groupes séparés par un filet de 30 px |
| Poignée de sélection / d'objet | 9 × 9 px visibles (bord 1 px), poignée ambre bas droit 13 × 13 ; **zone de prise 24 × 24 px minimum** |
| Loupe | 15 × 15 pixels physiques × 8 = 120 px, pixel central cerclé, marge 8 px, lecture hex + coordonnées |
| HUD (aide, dimensions) | marge 10 × 14 px, touches 22 px de haut, largeur max 1080 px (passe à la ligne) |
| Miniature | 300 px de large, aperçu 284 × 160, actions 48 px de haut, fermeture 28 px visibles / 44 px de zone |
| Pastille de couleur (sélecteur) | 26 px visibles, 36 px de zone |
| Compteur (dans l'image) | diamètre 22 / 28 / 36 px × échelle (Fine / Moyenne / Épaisse), chiffre gras 0,5 × diamètre, bord blanc 2 px |
| Épaisseurs d'annotation | 2 / 4 / 8 px × échelle de capture (défaut : Moyenne) |
| Éditeur, fenêtre minimale | 720 × 480 px |

**Rayons** : sélection, poignées, pixels de la loupe : **0** (angles vifs, comme le logo) ; HUD 8 ; dimensions 6 ;
touches 4 ; outil 8 ; miniature : celui de la plateforme (14 KDE, 12 macOS) ; fenêtres, contrôles, menus : feuille de
style système. **Ne jamais coder en dur un rayon de fenêtre** : sous macOS 26, AppKit applique Liquid Glass si
l'application est liée au SDK 26.

**Ombres** : HUD clair `0 8px 24px rgba(17,19,24,.28)` + filet `0 0 0 1px rgba(17,19,24,.10)` ; HUD sombre
`0 8px 24px rgba(0,0,0,.5)` + filet `rgba(255,255,255,.08)`. Fenêtres, dialogues, menus, notifications : système.

## 7. Mouvement

| Jeton | Durée, courbe | Usage | Mouvement réduit |
|---|---|---|---|
| `instant` | 0 | apparition de l'overlay (l'image figée doit coïncider avec l'écran) | 0 |
| `fast` | 120 ms, ease-out | survol, outil actif, poignée saisie | 0 |
| `base` | 200 ms, `cubic-bezier(.2,0,0,1)` | entrée de la miniature (glissement de 16 px + fondu), toast | fondu seul, ou rien |
| `exit` | 150 ms, ease-in | sortie de la miniature | 0 |

**Durée de la miniature** : réglage séparé, indépendant du mouvement réduit (M) : 5 s (défaut), 10 s, « Jusqu'à
fermeture ». Le décompte est en pause au survol et au focus ; il est désactivé avec VoiceOver actif.
Détection : `gtk-enable-animations` (GTK), `NSWorkspace.accessibilityDisplayShouldReduceMotion` (AppKit). Les
indicateurs indéterminés deviennent un texte d'état (« Déchiffrement… »), jamais une animation seule.

## 8. Composants

### 8.1 Overlay de sélection (à nous)

- Une surface par écran : `xdg_toplevel` plein écran (GNOME), layer-shell `overlay` (KDE, wlroots), `NSPanel` sans
  bord au-dessus de tout, exclu de la capture (macOS). Image figée par écran, à son échelle.
- **Voile** `k-veil` hors sélection ; la zone reste à 100 %. **Bord** blanc 1 px + liseré sombre extérieur 1 px.
- **Poignées** : 7 blanches + 1 ambre au coin bas droit (la poignée du logo), celle que déplace `Alt+flèches`.
  Zone de prise 24 px. **Petite zone** : sous 48 px (logiques) de côté, les poignées médianes disparaissent et les zones
  de prise des coins débordent vers l'extérieur ; l'intérieur reste la zone de déplacement. **Au bord de l'écran**, les
  poignées restent dans l'écran (décalées vers l'intérieur).
- **Pastille du haut** : « Écran figé · 14:03:22 · DP-1 · 2560 × 1440 · 200 % ».
- **Priorité et retournement des HUD** (I5) : la zone n'est jamais couverte par un HUD. Ordre : dimensions, puis
  loupe, puis aide. Dimensions sous la zone, au-dessus s'il manque de place, alignées sur le bord intérieur ; loupe
  près du curseur, du côté opposé au bord le plus proche ; aide en bas au centre, **repliée sur une pastille `? Aide`**
  dans le coin libre si elle couvrirait la zone. Aide dépliée aux trois premières utilisations, puis repliée.
- **Dimensions** : `1280 × 720 px · x 424 · y 372`, en **pixels physiques** (taille du PNG).

| Touche (Linux / macOS) | Action |
|---|---|
| flèches | déplacer le réticule, puis la zone, de 1 px physique |
| `Maj` / `⇧` + flèches | idem, par 10 px |
| `Alt` / `⌥` + flèches | redimensionner depuis la poignée ambre, 1 px (`Alt+Maj` / `⌥⇧` : 10 px) |
| `Ctrl+A` / `⌘A` | sélectionner l'écran actif entier (ajustable ensuite) |
| `R` | reprendre la dernière zone |
| `Tab` / `Maj+Tab` | écran suivant / précédent ; **absent s'il n'y a qu'un écran** |
| `Entrée` / `↩` | valider ; **sans zone : rien**, l'aide rappelle « Glissez ou Ctrl+A » |
| `E` | valider et ouvrir l'éditeur |
| `Échap` / `⎋` | annuler (code de sortie 3) |
| `?` | afficher / masquer l'aide |

- **Aide** : première ligne en gras = actions principales (avec zone : `Entrée` Valider, `E` Annoter, `Échap` Annuler ;
  sans zone : « Glissez pour sélectionner une zone », `Ctrl+A` Écran entier, `Échap` Annuler) ; seconde ligne =
  déplacements et options. L'entrée `Entrée` n'apparaît qu'avec une zone.
- **Multi-écran** : sélection limitée à un écran (échelles mixtes) ; l'autre écran affiche son nom, sa taille et
  « Tab pour sélectionner sur cet écran » ; `Tab` annonce « Écran HDMI-1, 1920 par 1080 ». Une zone poussée contre le
  bord affiche 2 s « La zone ne peut pas dépasser cet écran » (région `polite`).
- **Pas de 100 px** (décision de l'utilisateur) : un grand déplacement se fait par `Ctrl+A` puis `Alt+Maj+flèches`,
  ou à la souris. Saut au bord (`Début`/`Fin`) : non retenu, à rouvrir si les tests le demandent.

### 8.2 Barre d'outils de l'éditeur

Ordre et groupes : **V** Sélection | **A** Flèche, **R** Cadre, **O** Ellipse, **L** Ligne | **T** Texte, **P**
Crayon, **H** Surligneur | **N** Compteur | **B** Flou, **X** Pixellisation, **M** Masque | **C Recadrer (proposé)**.
Lettre visible sous chaque icône. Outil actif : `k-accent`, contenu `k-on-accent`, liseré `k-accent-text` en clair.
Focus clavier : anneau 2 px `k-text` décalé de 2 px, y compris sur l'outil actif. Les boutons restent natifs ; seuls
l'état actif et le focus des outils sont stylés.

- **Lettres fixes** (disposition anglaise, comme la plupart des éditeurs), **actives seulement quand l'éditeur a le
  focus et hors saisie de texte**. Elles sont aussi listées dans le menu Outils (macOS) et la fenêtre « Raccourcis
  clavier ».
- **Sélection d'objet** (I6) : un clic sans glisser (< 4 px) sur un objet le sélectionne, **quel que soit l'outil** ;
  un glisser dessine, même par-dessus un objet. Les poignées de l'objet sélectionné se saisissent avec n'importe quel
  outil. L'outil **V** sélectionne et déplace (glisser = déplacer l'objet). Clic dans le vide : désélectionne.
- **Recadrer (C) — proposé, en attente de décision de l'utilisateur.** Promis par `brand.md` (§ 7), absent de
  `docs/plan.md`. Icône **système** (Adwaita symbolic de recadrage, nom exact à vérifier ; SF Symbols `crop`), jamais le
  symbole Kropa. Si retenu : cadre de recadrage avec les poignées de l'overlay, `Entrée` applique, `Échap` annule,
  dimensions en pixels physiques ; non destructif jusqu'à l'enregistrement. Si refusé : le bouton disparaît, la lettre
  `C` reste libre. Dans la maquette, il porte un contour pointillé qui n'existera pas dans le produit.
- **Largeur** : la barre tient en 700 px (13 outils) ; sous 760 px les écarts passent à 2 px et le zoom passe dans le
  menu ; sous 700 px, les groupes de droite passent dans un menu « Plus d'outils » (non dessiné).

### 8.3 Panneau de style

`AdwOverlaySplitView` (côté fin, `F9`) / inspecteur `NSSplitViewController` (`⌥⌘I`, au lieu de `⌃⌘S`, M). Le titre
dit l'objet ou l'outil (« Flèche sélectionnée », « Texte »). Contenu : couleur (7 pastilles nommées), épaisseur (Fine
/ Moyenne / Épaisse) ou taille du texte (Petite / Moyenne / Grande), contour de lisibilité ou fond de couleur du
texte ; flou : intensité (Moyenne / Forte, défaut Forte) et avertissement « un flou peut être inversé ; masque pour un
secret » avec bouton « Utiliser le masque (M) ». En bas : **liste des annotations** (ordre de `Tab`).

### 8.4 Annotations

- **Poignées d'objet** : comme celles de l'overlay (9 px, zone 24 px) ; extrémités pour flèches et lignes, quatre
  coins pour les boîtes ; boîte de sélection en tirets 1 px `#111318` doublés de blanc.
- **Compteurs** : disque plein de la couleur, chiffre gras centré (tableau 4.4), bord blanc ; numérotation continue,
  renumérotée à la suppression. Le mot est « Compteur » partout (outil, liste, lecteur d'écran).
- **Masque** : rectangle **opaque** (défaut `#15171C`), sans transparence possible : seul masquage sûr.
- **Flou** : gaussien, rayon fort 24 px × échelle (moyen 12) ; pas de flou faible. **Pixellisation** : blocs de
  16 px × échelle (minimum 12). L'aperçu est le rendu final. Valeurs à confirmer en 3.2.
- **Surligneur** (I8) : rectangle jaune `#F6D32D` à **45 % d'opacité, mélange normal** (lisible sur fond clair et
  sombre) ; `multiply` exclu.
- **Texte** : Inter embarquée (§ 5), fond de couleur facultatif (texte blanc ou noir selon la couleur). **Saisie** :
  cadre pointillé, curseur visible ; les lettres d'outils sont inactives ; `Entrée` = nouvelle ligne ; `Échap` ou clic
  ailleurs termine ; un texte vide est supprimé.
- **Noms accessibles** (I15) : type, couleur, taille, position en pixels physiques (« Flèche rouge, moyenne, de 708,
  520 à 1040, 640 » ; « Compteur 2, rouge, en 1028, 586 »). Visible dans la liste : nom court + détail. Un seul numéro
  visible : celui du compteur (pas de colonne de rang).

### 8.5 Clavier de l'éditeur

Même règle que l'overlay : flèches 1 px, `Maj` ×10, `Alt` / `⌥` redimensionne. `Tab` / `Maj+Tab` entre annotations,
`Suppr`, `Ctrl+Z` / `Ctrl+Maj+Z` (`⌘Z` / `⇧⌘Z`), `Ctrl+C` copie l'image, `Ctrl+S`, `Ctrl+U` envoie, `Ctrl+0` taille
réelle. **Choix de la destination** : `⇧⌘U` sous macOS ; **aucun raccourci sous Linux** (`Ctrl+Maj+U` est la saisie
Unicode de GTK, M) : le bouton fléché se prend au clavier. **`Échap`** annule le tracé en cours, termine la saisie,
puis désélectionne ; **il ne ferme jamais la fenêtre**. Fermer : `Ctrl+W` / `⌘W` (confirmation si non enregistré).
**`F6`** fait le tour barre d'outils → canevas → panneau → barre d'en-tête. **`Ctrl+?`** ouvre « Raccourcis clavier »
(`AdwShortcutsDialog`, libadwaita 1.8, sinon `GtkShortcutsWindow`) ; macOS : Aide › Raccourcis clavier.

### 8.6 Fenêtres natives

- **Réglages** : GNOME `AdwApplicationWindow` + `AdwViewSwitcher` (Général, Destinations, Raccourcis) ;
  sous-pages `AdwNavigationView` ; < 400 px : `AdwViewSwitcherBar`. macOS : fenêtre Réglages à onglets (`⌘,`),
  sous-tâches en feuille. Tâche durable = fenêtre ; tâche courte (coffre, confirmation) = dialogue (M : cohérence).
- **Général** : section Confidentialité en **description de groupe** (M), pas en rangées ; réglage « Miniature après
  capture » (5 s, 10 s, Jusqu'à fermeture).
- **Destinations** : liste, défaut marqué par une étiquette **neutre avec coche**, « Secret manquant » icône + texte,
  `Tester…` sur chaque rangée. Boutons des rangées **plats** sous libadwaita (M). **Test** : confirmation qui nomme
  l'hôte, puis résultat : succès (statut, durée, URL, lien formaté, suppression si l'API le permet) ou **échec**
  (« Échec : réponse 401 en 120 ms », cause, « Remplacer le jeton… », « Voir le journal », « Tester à nouveau »).
- **Import `.sxcu`** : hôte en entier (jamais tronqué), **forme ASCII (punycode) à côté quand elle diffère** et
  avertissement sur les noms d'apparence identique, origine et HTTPS, requête, secrets détectés masqués avec leur
  magasin et leur référence, champs non repris, avertissement « fichier en clair, à supprimer ». Les 4 derniers
  caractères ne s'affichent que si le secret en compte au moins 16 (sinon entièrement masqué, M). Le bouton répète
  l'hôte (« Importer vers img.café.example ») et **passe à la ligne**. **`http://` avec secret** : refusé avant cet
  écran, dialogue « Import refusé » qui dit pourquoi et comment corriger ; rien n'est enregistré.
- **Raccourcis** : GNOME délègue au portail GlobalShortcuts ; wlroots : « indisponible » + ligne `bindsym` ; macOS :
  enregistreur qui refuse `⌥` seul et `⌥⇧`, garde l'ancien raccourci, explique.
- **Coffre `age`** : `AdwAlertDialog` + `GtkPasswordEntry` (œil nommé « Afficher la phrase de passe ») / `NSAlert` +
  `NSSecureTextField` (**pas d'œil**, M). Sans fenêtre parente, le dialogue s'ouvre dans sa propre fenêtre (à vérifier
  sur la version de libadwaita ciblée). Dit pourquoi le coffre, où est le fichier, durée d'ouverture (15 min,
  verrouillage d'écran), commande `kropa secrets unlock`. **Création** : deux saisies, correspondance annoncée,
  avertissement « Kropa ne peut pas retrouver une phrase de passe oubliée ».
- **File d'attente** (« File d'attente » partout, M) : nom (ellipse au milieu), destination, taille, statut icône +
  texte, une action utile (Annuler, Déverrouiller…, Réessayer maintenant, Remplacer le jeton…, Copier le lien).
  Statut occupé annoncé en région `polite`, au plus toutes les 5 s (M).
- **Premier lancement** : 4 étapes (bienvenue, permission, raccourci, prêt), une permission à la fois, expliquée
  avant le dialogue du système.
  - **Refus GNOME** (I12) : « GNOME a mémorisé votre refus et ne reposera pas la question. » Procédure : 1) effacer le
    refus du magasin d'autorisations des portails, commande à copier `flatpak permission-reset <app-id>` ; hors Flatpak,
    `kropa doctor` affiche la commande adaptée ; 2) « Réessayer ». Aucun chemin de Paramètres GNOME n'est promis.
  - **Sway et wlroots sans portail** : « Aucune autorisation à donner » (screencopy) ; liste des capacités détectées
    (screencopy, layer-shell, portail absent mais inutile, raccourcis globaux indisponibles) et `kropa doctor`.
  - **macOS** : chemin des Réglages Système, remède « retirer puis rajouter », rappel mensuel dit franchement.

### 8.7 Menu résident et lanceur (I11)

Contenu commun, dans cet ordre : **Capturer** (zone, écran, dernière zone, avec les raccourcis globaux réglés) ;
**Dernière capture** (heure et taille en titre ; ouvrir dans l'éditeur, copier l'image, copier le lien) ; **File
d'attente : N envois…** ; **Déverrouiller le coffre…** seulement si un envoi l'attend ; **Réglages** ; **Quitter
Kropa**. L'état se lit en texte dans le menu, jamais par un badge seul ; le nom accessible de l'icône le reprend
(« Kropa, 2 envois dans la file »).

| Plateforme | Accès | Rendu |
|---|---|---|
| macOS | `NSStatusItem`, symbole Kropa en image gabarit ; `LSUIElement` | `NSMenu` ; flèches au clavier ; VoiceOver « Kropa, élément de barre d'état » |
| macOS, fenêtre ouverte | `NSApp.setActivationPolicy(.regular)` à l'ouverture d'une fenêtre, `.accessory` à la fermeture de la dernière | Dock et barre de menus : Kropa, Fichier, Édition, Présentation, **Outils** (lettres, inspecteur, raccourcis), Fenêtre, Aide |
| KDE Plasma | StatusNotifierItem (ksni) | clic gauche : capturer une zone ; clic droit / touche Menu : le menu (dessiné par Plasma) ; « Configurer Kropa… » |
| GNOME | **pas d'icône sans extension** | raccourci global ; **lanceur** : ouvrir Kropa depuis la grille d'applications affiche une petite fenêtre (capturer, dernière capture, file, réglages) qui **se masque avant chaque capture** ; actions du `.desktop` au clic droit sur l'icône (Capturer une zone, Capturer l'écran, Ouvrir le lanceur) ; actions des notifications. Flatpak : listé dans « Applications en arrière-plan » (portail Background) ; paquet natif : à vérifier |

### 8.8 Notifications, toast et miniature

| Situation | Titre | Corps | Actions |
|---|---|---|---|
| Lien copié | Lien copié | URL · « Enregistrée dans ~/Images/Kropa. » | Ouvrir, Annoter |
| GNOME, lien obtenu **sans fenêtre Kropa au premier plan** | Lien prêt | URL · « GNOME ne permet pas de copier sans votre clic. » | Copier le lien, Ouvrir |
| Échec passager (5xx) | Envoi échoué | « Zipline a répondu 503… La capture est enregistrée en local et reste dans la file d'attente. » | Réessayer, Voir le journal |
| Jeton refusé (401) | Envoi échoué | « … a refusé le jeton (401)… » | Remplacer le jeton…, Voir le journal |
| Accès refusé (403) | Envoi échoué | « … a refusé l'envoi (403)… » | Voir le journal |
| Réseau | Envoi en attente | « img.maison.lan ne répond pas. Nouvel essai dans 30 s. » | Réessayer maintenant |
| Coffre verrouillé | Envoi en attente : coffre verrouillé | « 2 captures attendent le jeton de img.maison.lan. » | Déverrouiller… |

- **Fin d'envoi depuis l'éditeur** (I7) : la fenêtre a le focus, donc copie directe ; GNOME `AdwToast` « Lien copié ·
  URL · Ouvrir » ; macOS : barre d'information sous la barre d'outils. **L'éditeur reste ouvert** ; le sous-titre dit
  « Envoyée vers Zipline maison · lien copié ». Jamais « Lien copié » avant la copie réelle (`clipboard.status` :
  `pending` → `copied`).
- **Miniature** (I10) : aperçu, « Capture enregistrée », Annoter (`Entrée`), Copier (`Ctrl+C` / `⌘C`), Envoyer
  (`Ctrl+U` / `⌘U`), décompte en `k-accent-text`. Elle ne vole pas le focus. Les **raccourcis ne s'affichent que
  quand le panneau a le focus clavier** (pas au survol). Au survol : pause, bouton fermer (28 px visibles, 44 px de
  zone). Annonce à l'apparition (`NSAccessibility` : « Capture enregistrée, 1280 par 720 »). Avec VoiceOver actif, ou
  le réglage « Jusqu'à fermeture », pas de fermeture automatique. Raccourci global **« Ouvrir la dernière capture »**
  (lot 4.3) + commande CLI à ajouter à `docs/cli.md` (proposition : `kropa edit --last`).
- **GNOME** : notification avec les mêmes actions ; **petit aperçu à vérifier en prototype** (portail Notification).

## 9. États

| État | Règle |
|---|---|
| Survol | fond `--btn` natif ; poignées : curseur système de redimensionnement |
| Focus visible | fenêtres : anneau natif (libadwaita : accent à 50 %, ≈ 2:1 sur blanc, **écart système documenté**, I13) ; outils : 2 px `k-text` décalés de 2 px, visible aussi sur l'outil actif ; overlay et miniature : 2 px `k-accent-text` ; jamais retiré |
| Actif / pressé | outil : ambre + liseré ; autres : style natif « checked » ; exposé à l'accessibilité |
| Inactif | style natif ; rangée : titre seul à 0,55, raison en `k-muted` plein (§ 4.3) |
| Chargement | bouton occupé avec libellé (« Envoi… », « Déchiffrement… ») + barre déterminée si la progression est connue ; annonce `polite` au plus toutes les 5 s |
| Erreur | phrase complète, cause + conséquence + action ; champ en erreur entouré ; bandeau (`AdwBanner`) dans l'éditeur |
| Succès | toast dans la fenêtre au premier plan, notification sinon, ou état dans la rangée (icône + texte) ; jamais de fanfare |
| Vide | page d'état (`AdwStatusPage`) : ce qui manque, pourquoi ce n'est pas grave, action, commande CLI |
| Texte long | noms de fichier et URL : ellipse au milieu, valeur entière en infobulle et description accessible ; **hôtes jamais tronqués** ; boutons qui portent un hôte : retour à la ligne |
| Petit écran | réglages adaptatifs (360 px) ; éditeur minimal 720 × 480 : panneau de style replié (ouvert en superposition), Enregistrer en icône, zoom dans le menu, titre décalé plutôt que superposé |

## 10. Accessibilité

- **Clavier complet** : overlay (§ 8.1), éditeur (§ 8.5), fenêtres (ordre de tabulation = ordre de lecture). macOS :
  documenter « Navigation au clavier » (Réglages Système) pour atteindre tous les contrôles par `Tab`.
- **Lecteurs d'écran** : GTK `GtkAccessible` → AT-SPI ; AppKit `NSAccessibility` (VoiceOver). Outils : rôle bouton
  bascule, nom « Flèche (A) », « Recadrer (C, proposé) » dans la maquette, état pressé ; barre : rôle `toolbar`.
  Canevas : rôle image + liste des annotations exposée comme liste (noms § 8.4). Overlay : annonce polie (« Zone 1280
  par 720 pixels, position 424, 372 ») 500 ms après le dernier mouvement ; changement d'écran annoncé ; annonces
  assertives pour les erreurs (bandeau, dialogue).
- **Contraste** : texte ≥ 4,5:1, éléments graphiques et états ≥ 3:1 (tableaux § 4) ; écarts système signalés § 4.2 et
  § 9 (focus libadwaita).
- **Couleur** : jamais seule (états, pastilles, outil actif, défaut, état du résident).
- **Cibles** (I9) : **24 px minimum de zone de prise partout** (WCAG 2.5.8), poignées comprises ; 44 px sur nos
  surfaces (outils 44 × 52, actions de la miniature 48, fermeture 44) ; pastilles 36 px ; dans les fenêtres, taille
  native des contrôles (34 px GNOME, 22–24 px macOS), écart assumé au nom du natif.
- **Préférences système** : contraste élevé (libadwaita `high-contrast`, macOS « Augmenter le contraste ») : bord de
  sélection 2 px, voile 0,7, liseré de l'outil actif aussi en sombre ; grands textes GNOME (× 1,25) tenus par les mises
  en page, HUD compris ; mouvement réduit § 7.

## 11. Écarts par plateforme

| Sujet | libadwaita (GNOME HIG) | AppKit (Apple HIG) |
|---|---|---|
| Barre de titre | `AdwHeaderBar`, titre centré (décalé s'il manque de place), fermer à droite | barre unifiée `NSToolbar`, feux à gauche, titre et sous-titre à gauche |
| Action principale | `suggested-action` à droite de l'en-tête | bouton par défaut (`↩`) ; en bas à droite des feuilles |
| Accent | accent système de l'utilisateur, non surchargé | `AccentColor` ambre profond (exception § 3) |
| Réglages | `AdwViewSwitcher` + `AdwPreferencesPage`, adaptatif | onglets de barre d'outils, taille fixe, `⌘,` |
| Listes | `boxed-list`, rangées 50 px, rayon 12, boutons plats | sections groupées, rangées ~38 px, rayon 8, boutons à contour |
| Dialogues | `AdwAlertDialog`, boutons côte à côte, Annuler à gauche | `NSAlert` : icône, texte centré, boutons empilés, défaut en haut |
| Sous-tâche (test, import) | sous-page `AdwNavigationView` avec Retour | feuille attachée à la fenêtre |
| Erreur dans l'éditeur | bandeau `AdwBanner` | barre d'information maison sous la barre d'outils |
| Succès dans l'éditeur | `AdwToast` | barre d'information |
| Panneau de style | `AdwOverlaySplitView`, `F9` | inspecteur `NSSplitViewController`, `⌥⌘I` |
| Choix de destination | bouton fléché, sans raccourci | `⇧⌘U` |
| Raccourcis clavier | `AdwShortcutsDialog`, `Ctrl+?` | Aide › Raccourcis clavier ; menu Outils |
| Résident | KDE : StatusNotifierItem ; GNOME : lanceur, actions `.desktop`, notifications | `NSStatusItem` + `NSMenu` ; Dock et barre de menus quand une fenêtre est ouverte |
| Notifications | portail Notification ; GNOME Shell / Plasma dessinent | `UNUserNotificationCenter` ; actions au survol (« Options ») |
| Lien après envoi | fenêtre au premier plan : copié + toast ; sinon GNOME « Lien prêt — Copier le lien » ; KDE : copié | copié (`NSPasteboard`) |
| Miniature | GNOME : notification ; KDE, wlroots : layer-shell bas droit | `NSPanel` non activant, bas droit, tous les Spaces |
| Raccourcis globaux | portail GlobalShortcuts (GNOME confirme) ; wlroots : ligne `bindsym` | enregistreur dans Kropa ; `⌥` et `⌥⇧` seuls refusés |
| Notation des touches | `Ctrl+Maj+4` (FR), `Ctrl+Shift+4` (EN), touches dessinées | `⌃⇧4`, symboles sans séparateur, ordre ⌃⌥⇧⌘ |
| Raccourci de capture par défaut | `Ctrl+Maj+4` | `⌃⇧4` (évite `⌘⇧4` du système ; conflits à vérifier) |
| Modificateurs (overlay, éditeur) | `Maj` ×10, `Alt` redimensionner, `Ctrl+A` écran | `⇧` ×10, `⌥` redimensionner, `⌘A` écran |
| Permission de capture | portail Screenshot : dialogue une fois, refus mémorisé, effacé par la commande du permission store | TCC : Réglages Système, relance possible, rappel mensuel |
| Police | Adwaita Sans 11 pt, Adwaita Mono | SF Pro 13 px, SF Mono |
| Hauteur des contrôles | 34 px | 22–24 px |
| Focus | anneau 2 px, accent à 50 % (≈ 2:1, écart système) | anneau système, couleur dérivée de l'accent |
| Champ secret | `GtkPasswordEntry`, œil nommé | `NSSecureTextField`, sans œil |
| Icônes standard | Adwaita symbolic (`edit-undo-symbolic`, `edit-copy-symbolic`, `document-save-symbolic`, `send-to-symbolic`, `open-menu-symbolic`, `sidebar-show-right-symbolic`, `dialog-password-symbolic`, recadrage : nom à vérifier) | SF Symbols (`arrow.uturn.backward`, `doc.on.doc`, `square.and.arrow.down`, `square.and.arrow.up`, `sidebar.right`, `lock`, `crop`) |
| Icônes d'outils | SVG symboliques recolorés | PDF gabarit (`…Template`) |

## 12. Icônes d'outils (D2.2)

Fichiers `docs/design/icons/kropa-tool-{select,arrow,rectangle,ellipse,line,text,pencil,highlighter,counter,blur,pixelate,mask}-symbolic.svg`.

- Grille 20 px, marge 2 px, **contours pleins** qui reproduisent un trait de 1,75 px (GTK ne recolore de façon fiable
  que les aplats), angles vifs, extrémités carrées, une couleur `#2e3436`.
- Métaphores : pointeur plein (sélection, comme les pointeurs système), flèche à chevron d'équerre (rappel du logo),
  cadre, ellipse, ligne, T à empattements géométriques, crayon à pointe pleine, marqueur biseauté + trait de
  surlignage, cercle « 1 », points de taille croissante vers le centre (flou), mosaïque en X (pixellisation), texte
  caviardé (masque).
- **Recadrer** : icône système, pas de fichier Kropa (`brand.md` § 7).
- `<title>` neutre (nom de fichier, M) : le nom lisible vient du bouton, dans la langue de l'interface.
- Affichage à **20 px** (`-gtk-icon-size: 20px` ; taille de point 20 sous AppKit). À 16 px le trait tombe à 1,4 px :
  à éviter.
- Installation GTK : ressource `…/icons/scalable/actions/` ; macOS : export PDF gabarit au build.

## 13. Rédaction

Ton de `brand.md` (phrase courte, vouvoiement, pas de point d'exclamation, mêmes termes : capture, envoi,
destination, **file d'attente**, coffre, **compteur**, **bandeau**). Une action, un libellé : « Remplacer le jeton… »
partout (réglages, file, notification). Les chaînes des maquettes servent de base à D2.4 (`.ftl`) ; espaces
insécables du français, formats de date et **Title Case des boutons et menus en anglais** (déjà appliqué au menu
résident) à reprendre par `copywriter` en D2.4.

## 14. Points ouverts et décisions à soumettre

1. **Recadrer (C)** : à ajouter au plan (lot 3.x) ou à retirer de la barre ? Proposé ici, en attente.
2. **AccentColor macOS** ambre profond : exception écrite § 3 ; à reporter dans `brand.md` § 6 après accord.
3. **« Remplacer le jeton… »** (critique M) remplace « Mettre à jour le jeton… » (validé en réponse 5) pour n'avoir
   qu'un libellé : à confirmer par l'utilisateur ou `copywriter`.
4. **macOS** : passage en `.regular` (Dock et barre de menus) quand une fenêtre est ouverte, à confirmer avec le lot 4.2.
5. **GNOME** : lanceur en fenêtre + actions `.desktop` ; apparition dans « Applications en arrière-plan » hors
   Flatpak, aperçu dans la notification : **à vérifier en prototype**.
6. Commande CLI « Ouvrir la dernière capture » (`kropa edit --last` proposé) : à ajouter à `docs/cli.md`.
7. `AdwDialog` sans parent (coffre déclenché par une notification) : comportement à vérifier sur la version de
   libadwaita ciblée ; repli : petite `AdwWindow`.
8. Nom Adwaita de l'icône de recadrage : à vérifier dans le thème installé.
9. Raccourci macOS par défaut `⌃⇧4` : conflits à vérifier sur le Mac de test.
10. Saut au bord (`Début`/`Fin`) dans l'overlay : non retenu, à rouvrir après tests.

## 15. Mineurs de la critique D2.3

Traités : œil du champ secret (retiré sous AppKit, nommé sous GNOME) ; cohérence fenêtre / dialogue ; un seul libellé
« Remplacer le jeton… » ; « File d'attente » partout ; boutons plats dans les rangées ; notifications sans accent ;
zoom en double retiré (le canevas n'a plus d'indicateur) ; flou de la maquette au niveau « Fort » ; lettres d'outils
fixes documentées (focus, hors saisie) ; `Ctrl+Maj+U` retiré sous Linux, `⌃⌘S` → `⌥⌘I` ; mouvement réduit séparé de la
durée ; `k-sel-edge` et `k-handle` fusionnés, `#A84B00` remplacé par `k-accent-text` ; texte d'aide GNOME de « Copier
l'image » reformulé ; miniature GNOME « à vérifier en prototype » ; `<title>` des SVG neutres ; HUD qui suit l'échelle
du texte ; statut occupé `polite` par pas de 5 s ; aucune ressource externe dans les maquettes (polices système) ;
Confidentialité en description de groupe ; seuil de 16 caractères pour afficher les 4 derniers.
Reporté en D2.4 : Title Case anglais de toutes les chaînes (seul le menu résident est déjà en Title Case).
