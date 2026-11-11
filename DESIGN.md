# DESIGN.md — Kropa

> Direction visuelle et règles d'interface, à respecter par toute interface (lots 2.3, 2.4, 3.5, 3.6, 4.10, 4.11).
> État : **proposition D2.1 / D2.2 du 11/11/2026**, à critiquer en D2.3 (`design-critic`), puis à valider par
> l'utilisateur. Sources : `docs/decisions.md` (prioritaire), `docs/brand/brand.md` (identité A « Repère », validée),
> `docs/product.md`, `docs/cli.md`, `docs/platform-linux.md`, `docs/platform-macos.md`, `docs/security/secrets.md`.
> Maquettes : `docs/design/mockups.html` (8 écrans + icônes ; libadwaita / AppKit, clair / sombre, FR / EN, états).
> Icônes d'outils : `docs/design/icons/`.

## 1. Cadre

- **Public** : Léa (développeuse, GNOME Wayland), Marc (admin, KDE), puis Sophie et Karim (macOS, Ubuntu).
- **Tâche principale** : zone → annotation → lien, au clavier, en moins de trois touches hors annotation.
- **Plateformes** : GTK 4 + libadwaita (GNOME HIG) sous Linux ; AppKit (Apple HIG) sous macOS 15+.
- **Contraintes** : clair et sombre, FR et EN, tout au clavier, contraste texte ≥ 4,5:1, jamais de succès simulé,
  aucune requête réseau non configurée.

## 2. Principes

1. **Natif d'abord.** Fenêtres, contrôles, dialogues, notifications et polices sont ceux du système. On ne dessine
   pas de thème Kropa par-dessus libadwaita ni AppKit.
2. **L'identité vit là où l'écran nous appartient** : l'overlay de sélection, l'outil actif, les icônes d'outils,
   la miniature, l'icône d'application. C'est là que sert l'ambre.
3. **Précis.** Pixels physiques affichés, loupe au pixel, poignées à angles vifs, chiffres tabulaires.
4. **Loyal.** Chaque état dit ce qui s'est passé, où est le fichier, quoi faire ; une limite de plateforme se dit
   (GNOME, wlroots, macOS), elle ne se cache pas.
5. **Sobre.** Une action principale par écran, pas d'animation gratuite, densité de bureau.

## 3. Partage du territoire

| Élément | Qui le dessine | Ce que Kropa y met |
|---|---|---|
| Overlay de sélection, loupe, aide clavier | Kropa (surface plein écran ou layer-shell, `NSPanel`) | tout : voile, sélection, poignée ambre, HUD |
| Barre d'outils de l'éditeur | widgets natifs (`GtkToggleButton`, `NSButton`) | icônes maison, lettre visible, état actif ambre |
| Canevas et annotations | Kropa (`kropa-render`) | palette d'annotation, pastilles, masques |
| Miniature (macOS, KDE, wlroots) | conteneur natif, contenu Kropa | aperçu, actions, décompte ambre |
| Fenêtres (réglages, file, coffre, premier lancement) | 100 % natif | textes, ordre, états ; aucune couleur de marque |
| Notifications | système (portail Notification, `UNUserNotificationCenter`) | textes et actions seulement |

## 4. Couleurs

### 4.1 Jetons Kropa (surfaces à nous)

Repris de `brand.md` (valeurs inchangées, contrastes mesurés par l'identité) ; jetons ajoutés marqués « + ».
Contrastes calculés selon WCAG 2.x (luminance relative sRGB).

| Jeton | Clair | Sombre | Rôle | Contraste mesuré |
|---|---|---|---|---|
| `k-surface` | `#FFFFFF` | `#1A1D24` | fond du HUD (aide, dimensions, loupe), miniature KDE | — |
| `k-raised` | `#F0F1F4` | `#262A33` | touches (`kbd`) dans le HUD | — |
| `k-line` | `#D9DCE2` | `#3A3F4A` | séparateurs, grille de la loupe (décoratif) | — |
| `k-line-strong` | `#7D8490` | `#7A818D` | contour de la loupe, bas de touche | 3,77:1 / 4,30:1 (graphique ≥ 3) |
| `k-text` | `#15171C` | `#ECEEF2` | texte du HUD | 17,93:1 / 14,52:1 sur surface |
| `k-muted` | `#5A606B` | `#A2A8B3` | libellés secondaires du HUD **et** texte secondaire informatif des fenêtres (§ 4.3) | 6,32:1 / 7,06:1 sur surface |
| `k-accent` | `#FF8A1F` | `#FF8A1F` | remplissage : outil actif, poignée, décompte | 7,88:1 avec `#111318` posé dessus ; 2,36:1 sur blanc (jamais seul) |
| `k-on-accent` | `#111318` | `#111318` | icône et lettre sur l'ambre | 7,88:1 |
| `k-accent-text` | `#A84B00` | `#FF8A1F` | liseré de l'outil actif en clair, anneau de focus de nos surfaces | 5,72:1 sur blanc / 7,15:1 sur `#1A1D24` |
| + `k-veil` | `rgba(17,19,24,.55)` | idem | voile hors sélection (constant : il couvre l'écran de l'utilisateur) | sans texte |
| + `k-sel-edge` | `#FFFFFF` + liseré `rgba(17,19,24,.65)` | idem | bord de sélection et poignées | blanc/`#111318` 18,58:1 → au moins 4,31:1 contre tout fond |
| + `k-handle` | `#FFFFFF`, bord `#111318` | idem | poignées de sélection et d'objet | idem |
| `k-success` / `k-danger` | `#1A7F45` / `#C62828` | `#4ADE80` / `#FF6B6B` | icônes d'état de nos surfaces | 5,04:1 / 5,62:1 sur blanc |

Contrôles complémentaires : liseré `#A84B00` de l'outil actif contre la barre blanche 5,72:1 ; ambre contre la barre
sombre GNOME `#2E2E32` 5,73:1 et la fenêtre sombre macOS `#2C2C2E` 5,91:1 (pas de liseré nécessaire en sombre) ;
`k-text` sur `k-raised` clair 15,88:1, sombre 12,37:1.

### 4.2 Accent et couleurs système (fenêtres)

- **GNOME** : on **n'écrase pas** l'accent système (choix de l'utilisateur depuis GNOME 47). Boutons `suggested-action`,
  interrupteurs, sélection, liens : couleurs libadwaita (`@accent_bg_color`, `@accent_color`…).
- **macOS** : la ressource `AccentColor` de l'app vaut **ambre profond** `#A84B00` (clair) / `#B85400` (sombre).
  macOS ne l'applique que si l'utilisateur a laissé l'accent sur « Multicolore » (réglage par défaut) ; sinon son
  choix l'emporte. Texte blanc dessus : 5,72:1 / 4,88:1. L'ambre vif `#FF8A1F` est exclu ici : le système y pose du
  texte blanc (2,36:1).
- **Mesure honnête** : le bouton principal par défaut des plateformes reste sous 4,5:1 quand l'utilisateur garde
  le bleu (GNOME `#3584E4` + blanc gras 3,77:1 ; macOS `#007AFF` + blanc 4,02:1). C'est le rendu système, pas
  modifié par Kropa ; à signaler à la critique.
- **Couleur d'état** : la couleur ne porte jamais seule un état. Le texte d'état garde la couleur de texte
  principale ; l'icône porte la couleur (`success`, `error`, `warning` du système). Exception : message d'erreur
  sous un champ (GNOME `@error_color` `#C30000` 6,06:1 sur `#FAFAFB`, sombre `#FF938C` 7,40:1 sur `#222226` ;
  macOS : texte principal + icône rouge, car `systemRed` fait 3,6:1 sur blanc).

### 4.3 Texte secondaire

`.dim-label` de libadwaita (opacité 0,55, ≈ 3,2:1 sur blanc) et `secondaryLabelColor` de macOS (≈ 4:1) sont
**réservés au texte non essentiel**. Une information (chemin, hôte, état, raccourci) utilise `k-muted` :
`#5A606B` = 6,06:1 sur `#FAFAFB` (GNOME) et 5,35:1 sur `#ECECEC` (macOS) ; `#A2A8B3` = 6,63:1 sur `#222226`
(GNOME) et 5,83:1 sur `#2C2C2E` (macOS). Mise en œuvre : une classe CSS `.kropa-muted` sous GTK (ajoutée par un
fournisseur de style applicatif), une `NSColor` dynamique sous AppKit. En mode contraste élevé, on rend la main au
système.

### 4.4 Palette d'annotation (dessinée dans l'image)

| Nom | Hex | Contraste vs blanc / noir | Chiffre des pastilles |
|---|---|---|---|
| Rouge (défaut) | `#E01B24` | 4,83 / 4,35 | `#FFFFFF` (4,83:1) |
| Ambre | `#FF8A1F` | 2,36 / 8,91 | `#111318` (7,88:1) |
| Jaune | `#F6D32D` | 1,47 / 14,28 | `#111318` (12,63:1) |
| Vert | `#2EC27E` | 2,30 / 9,13 | `#111318` (8,08:1) |
| Bleu | `#1C71D8` | 4,77 / 4,40 | `#FFFFFF` (4,77:1) |
| Noir | `#15171C` | 17,93 / — | `#FFFFFF` (17,93:1) |
| Blanc | `#FFFFFF` | — / 21 | `#111318` (18,58:1) |

- **Contour de lisibilité** (activé par défaut) : liseré de 1,5 px (× échelle) blanc pour les couleurs sombres, `#15171C`
  pour les claires (luminance > 0,4) ; il garantit ≥ 3:1 contre le fond capturé.
- Le choix de couleur affiche une coche et un nom (« Rouge »), jamais la couleur seule.

## 5. Typographie

Interface : **police système uniquement**. Pas de police de marque dans l'application.

| Rôle | libadwaita (Adwaita Sans ; Cantarell avant GNOME 48) | AppKit (SF Pro) |
|---|---|---|
| Titre d'écran vide, premier lancement | `.title-1` 20 pt (26,7 px) 800 | 22 px 700 (title1 ≈ 22) |
| Titre de dialogue | `.title-2` 15 pt (20 px) 800 | 13 px 700 (`NSAlert`) |
| Titre de groupe, en-tête | `.heading` 11 pt (14,7 px) 700 | 13 px 700 |
| Texte courant | 11 pt (14,7 px) 400 | 13 px 400 |
| Sous-titre de rangée, légende | 9–10 pt (12–13 px) | 11 px |
| Lettre d'outil | 11 px 700 | 11 px 700 |
| Code, URL, chemins, hôtes | Adwaita Mono, `.monospace` | SF Mono |

- Chiffres tabulaires (`tnum`) pour dimensions, coordonnées, pourcentages, minuteries.
- **Texte des annotations** (dessiné dans l'image) : police embarquée pour un rendu identique sur les deux OS et des
  instantanés stables (lot 3.3). Proposition : Inter (OFL 1.1). À valider.
- Maquettes : Inter et JetBrains Mono (Google Fonts) imitent Adwaita Sans / Adwaita Mono et servent de repli à SF.

## 6. Espacements, dimensions, rayons, ombres

**Grille** : 4 px sur nos surfaces (`4, 8, 12, 16, 24, 32`). Dans les fenêtres, marges natives : multiples de 6 sous
libadwaita (6, 12, 18, 24 ; largeur de contenu 600 px, `AdwClamp`), 20 px de marge de fenêtre et 8 px entre
contrôles sous AppKit.

| Élément | Valeur |
|---|---|
| Outil de la barre | 44 × 52 px, icône 20 px, lettre 11 px dessous, 4 px d'écart, groupes séparés par un filet de 30 px |
| Poignée de sélection / d'objet | 9 × 9 px (bord 1 px) ; poignée ambre bas droit 13 × 13 px |
| Loupe | 15 × 15 pixels physiques × 8 = 120 px, pixel central cerclé, marge 8 px, lecture hex + coordonnées |
| HUD (aide, dimensions) | marge 10 × 14 px, touches 22 px de haut |
| Miniature | 300 px de large, aperçu 284 × 160, actions 48 px de haut, 5 s |
| Pastille numérotée | diamètre 22 / 28 / 36 px × échelle (Fine / Moyenne / Épaisse), chiffre gras 0,5 × diamètre, bord blanc 2 px |
| Épaisseurs d'annotation | 2 / 4 / 8 px × échelle de capture (défaut : Moyenne) |

**Rayons** : sélection, poignées, pixels de la loupe : **0** (angles vifs, comme le logo) ; HUD 8 ; dimensions 6 ;
touches 4 ; outil 8 (rayon d'un bouton natif) ; miniature : celui de la plateforme (14 KDE, 12 macOS) ; fenêtres et
contrôles : feuille de style système (libadwaita : fenêtre 12, bouton 6, liste 12, dialogue 18 ; macOS 15 : fenêtre
10, bouton 6). **Ne jamais coder en dur un rayon de fenêtre** : sous macOS 26, AppKit applique Liquid Glass si
l'application est liée au SDK 26.

**Ombres** : HUD clair `0 8px 24px rgba(17,19,24,.28)` + filet `0 0 0 1px rgba(17,19,24,.10)` ; HUD sombre
`0 8px 24px rgba(0,0,0,.5)` + filet `rgba(255,255,255,.08)`. Fenêtres, dialogues, notifications : ombre du système.

## 7. Mouvement

| Jeton | Durée, courbe | Usage | Mouvement réduit |
|---|---|---|---|
| `instant` | 0 | apparition de l'overlay (l'image figée doit coïncider avec l'écran) | 0 |
| `fast` | 120 ms, ease-out | survol, outil actif, poignée saisie | 0 |
| `base` | 200 ms, `cubic-bezier(.2,0,0,1)` | entrée de la miniature (glissement de 16 px + fondu) | fondu seul, ou rien |
| `exit` | 150 ms, ease-in | sortie de la miniature | 0 |
| décompte | 5 s, linéaire | miniature ; suspendu au survol et au focus | 10 s (proposition) |

Détection : `gtk-enable-animations` (GTK), `NSWorkspace.accessibilityDisplayShouldReduceMotion` (AppKit). Les
indicateurs indéterminés deviennent un texte d'état (« Déchiffrement… »), jamais une animation seule.

## 8. Composants

### 8.1 Overlay de sélection (à nous)

- Une surface par écran : `xdg_toplevel` plein écran (GNOME), layer-shell `overlay` (KDE, wlroots), `NSPanel` sans
  bord au-dessus de tout, exclu de la capture (macOS). Image figée par écran, à son échelle.
- **Voile** `k-veil` hors sélection ; la zone reste à 100 %. **Bord** blanc 1 px + liseré sombre extérieur 1 px.
- **Poignées** : 7 blanches + 1 ambre au coin bas droit (la poignée du logo), qui suit `Maj+flèches`.
- **Dimensions** sous la sélection (au-dessus si pas de place) : `1280 × 720 px · x 424 · y 372`, en **pixels
  physiques** (taille du PNG).
- **Loupe** : près du curseur, côté opposé au bord le plus proche ; bord de sélection tracé dans la grille.
- **Pastille du haut** : « Écran figé · 14:03:22 · DP-1 · 2560 × 1440 · 200 % ».
- **Aide clavier** en bas, au centre ; visible aux trois premières utilisations, puis repliée (`?` la rappelle).

| Touche (Linux / macOS) | Action |
|---|---|
| flèches | déplacer le réticule, puis la sélection, de 1 px physique |
| `Maj` / `⇧` + flèches | agrandir ou réduire depuis la poignée ambre |
| `Ctrl` / `⌥` + flèches | pas de 10 px (proposition ; `⌥` n'est refusé que pour les raccourcis globaux) |
| `R` | reprendre la dernière zone |
| `Tab` | écran suivant (proposition, multi-écran) |
| `Entrée` / `↩` | valider (sans sélection : écran entier sous le curseur, proposition) |
| `E` | valider et ouvrir l'éditeur |
| `Échap` / `⎋` | annuler (code de sortie 3) |
| `?` | afficher / masquer l'aide |

- **Multi-écran** : sélection limitée à un écran (échelles mixtes) ; l'autre écran affiche son nom, sa taille et
  « Tab pour sélectionner sur cet écran ». À trancher avec 2.2.

### 8.2 Barre d'outils de l'éditeur

Ordre et groupes : **A** Flèche, **R** Cadre, **O** Ellipse, **L** Ligne | **T** Texte, **P** Crayon, **H**
Surligneur | **N** Compteur | **B** Flou, **X** Pixellisation, **M** Masque (masquage isolé à droite, pour la
sécurité). Lettre visible sous chaque icône. Outil actif : `k-accent`, contenu `k-on-accent`, liseré `#A84B00` en
clair. Les boutons restent des boutons natifs ; seul l'état actif est stylé.

### 8.3 Panneau de style

`AdwOverlaySplitView` (côté fin, `F9`) / inspecteur `NSSplitViewController` (`⌃⌘S`). Contenu selon l'outil :
couleur (7 pastilles nommées), épaisseur (Fine / Moyenne / Épaisse), contour de lisibilité ; flou : intensité
(Moyenne / Forte, défaut Forte) et avertissement « un flou peut être inversé ; masque pour un secret » avec bouton
« Utiliser le masque (M) ». En bas : **liste des annotations** (ordre de `Tab`, accès lecteur d'écran).

### 8.4 Annotations

- **Poignées d'objet** : comme celles de l'overlay ; extrémités pour flèches et lignes, quatre coins pour les boîtes ;
  boîte de sélection en tirets 1 px `#111318` doublés de blanc.
- **Pastilles numérotées** : disque plein de la couleur, chiffre gras centré (couleur du tableau 4.4), bord blanc ;
  numérotation continue, renumérotée à la suppression.
- **Masque** : rectangle **opaque** (défaut `#15171C`), sans transparence possible : seul masquage sûr.
- **Flou** : gaussien, rayon fort 24 px × échelle (moyen 12) ; pas de flou faible. **Pixellisation** : blocs de
  16 px × échelle (minimum 12). L'aperçu est le rendu final. Valeurs à confirmer en 3.2.
- **Surligneur** : rectangle jaune, mode `multiply`, opacité 50 %.
- **Texte** : police embarquée (§ 5), fond facultatif (pastille de la couleur, texte contrasté).

### 8.5 Fenêtres natives

- **Réglages** : GNOME `AdwApplicationWindow` + `AdwViewSwitcher` (Général, Destinations, Raccourcis) ;
  sous-pages `AdwNavigationView` ; < 400 px : `AdwViewSwitcherBar` (`AdwBreakpoint`). macOS : fenêtre Réglages à
  onglets de barre d'outils (`⌘,`), sous-tâches en feuille.
- **Destinations** : liste, défaut marqué par une étiquette texte, « Secret manquant » icône + texte, `Tester…` sur
  chaque rangée. **Test** : confirmation qui nomme l'hôte, puis résultat (statut, durée, URL, lien formaté,
  suppression si l'API le permet).
- **Import `.sxcu`** : hôte en entier (jamais tronqué : il passe à la ligne), origine et HTTPS, requête, champ,
  extraction, secrets détectés masqués (4 derniers caractères) avec leur magasin et leur référence, champs non repris,
  avertissement « fichier en clair, à supprimer ». Le bouton répète l'hôte : « Importer vers xbackbone.example.net ».
- **Raccourcis** : GNOME délègue au portail GlobalShortcuts (« Modifier dans GNOME… ») ; wlroots : « indisponible »
  + ligne `bindsym Print exec kropa capture area` à copier ; macOS : enregistreur qui refuse `⌥` seul et `⌥⇧` à la
  saisie, garde l'ancien raccourci, explique.
- **Coffre `age`** : `AdwAlertDialog` / `NSAlert` + champ masqué ; dit pourquoi le coffre, où est le fichier, durée
  d'ouverture (15 min, verrouillage d'écran), commande `kropa secrets unlock`.
- **File d'attente** : nom (ellipse au milieu), destination, taille, statut icône + texte, une action utile
  (Annuler, Déverrouiller…, Réessayer maintenant, Mettre à jour le jeton…, Copier le lien).
- **Premier lancement** : 4 étapes (bienvenue, permission, raccourci, prêt), une permission à la fois, expliquée avant
  le dialogue du système.

### 8.6 Notifications et miniature

| Situation | Titre | Corps | Actions |
|---|---|---|---|
| Lien copié | Lien copié | URL · « Enregistrée dans ~/Images/Kropa. » | Ouvrir, Annoter |
| GNOME, lien obtenu | Lien prêt | URL · « GNOME ne permet pas de copier sans votre clic. » | **Copier le lien**, Ouvrir |
| Échec passager (5xx) | Envoi échoué | « Zipline a répondu 503… La capture est enregistrée en local et reste dans la file d'attente. » | **Réessayer**, Voir le journal |
| Échec d'authentification (401, 403) | Envoi échoué | « … a refusé le jeton (401)… » | **Mettre à jour le jeton**, Voir le journal |
| Réseau | Envoi en attente | « img.maison.lan ne répond pas. Nouvel essai dans 30 s. » | Réessayer maintenant |
| Coffre verrouillé | Envoi en attente : coffre verrouillé | « 2 captures attendent le jeton de img.maison.lan. » | **Déverrouiller…** |

Jamais « Lien copié » avant la copie réelle (`clipboard.status` : `pending` → `copied`). Miniature : aperçu,
« Capture enregistrée », Annoter (`Entrée`), Copier (`Ctrl+C` / `⌘C`), Envoyer (`Ctrl+U` / `⌘U`), décompte ambre ;
elle ne vole pas le focus. GNOME : notification avec petit aperçu et les mêmes actions.

## 9. États

| État | Règle |
|---|---|
| Survol | fond `--btn` natif ; poignées : curseur système de redimensionnement |
| Focus visible | anneau natif dans les fenêtres ; sur nos surfaces, 2 px `k-accent-text`, décalé de 2 px ; jamais retiré |
| Actif / pressé | outil : ambre + liseré ; autres : style natif « checked » ; exposé à l'accessibilité |
| Désactivé | style natif (opacité) ; une raison est donnée à côté si elle n'est pas évidente |
| Chargement | bouton occupé avec libellé (« Envoi… », « Déchiffrement… ») + barre déterminée si la progression est connue |
| Erreur | message en phrase complète, cause + conséquence + action ; champ en erreur entouré ; `AdwBanner` dans l'éditeur |
| Succès | notification, ou état dans la rangée (icône + texte) ; jamais de fanfare |
| Vide | page d'état (`AdwStatusPage`) : ce qui manque, pourquoi ce n'est pas grave, action, commande CLI |
| Texte long | noms de fichier et URL : ellipse au milieu, valeur entière en infobulle et description accessible ; **hôtes et origines jamais tronqués** dans les confirmations |
| Petit écran | réglages adaptatifs (360 px) ; éditeur minimal 720 × 480, panneau de style replié en superposition |

## 10. Accessibilité

- **Clavier complet** : overlay (§ 8.1) ; éditeur : lettres d'outil, `Tab` / `Maj+Tab` entre annotations, flèches
  1 px, `Maj+flèches` 10 px, `Suppr`, `Ctrl+Z` / `Ctrl+Maj+Z`, `Ctrl+C`, `Ctrl+S`, `Ctrl+U`, `Ctrl+Maj+U` (⌘ sous
  macOS), `F6` pour passer de la barre au canevas puis au panneau, `Échap` désélectionne puis ferme (avec confirmation
  si non enregistré). Fenêtres : ordre de tabulation = ordre de lecture. macOS : documenter « Navigation au clavier »
  (Réglages Système) pour atteindre tous les contrôles par `Tab`.
- **Lecteurs d'écran** : GTK `GtkAccessible` → AT-SPI ; AppKit `NSAccessibility` (VoiceOver). Outils : rôle bouton
  bascule, nom « Flèche (A) », état pressé ; barre : rôle `toolbar`. Canevas : rôle image + liste des annotations
  exposée comme liste. Overlay : annonce polie (« Zone 1280 par 720 pixels, position 424, 372 ») 500 ms après le
  dernier mouvement ; annonces assertives pour les erreurs (bannière, dialogue).
- **Contraste** : texte ≥ 4,5:1, éléments graphiques et états ≥ 3:1 (tableaux § 4) ; écart système signalé § 4.2.
- **Couleur** : jamais seule (états, pastilles de couleur, outil actif, défaut).
- **Cibles** : ≥ 44 px sur nos surfaces (outils 44 × 52, miniature 48, pastilles de couleur 36 px de zone avec 26 px
  visibles : à revoir) ; dans les fenêtres, taille native des contrôles (34 px GNOME, 22–24 px macOS), au-dessus du
  minimum WCAG 2.5.8 (24 px) mais sous 44 px : écart assumé au nom du natif, à critiquer.
- **Préférences système** : mode contraste élevé (libadwaita `high-contrast`, macOS « Augmenter le contraste ») :
  bord de sélection 2 px, voile 0,7, liseré de l'outil actif aussi en sombre ; grands textes GNOME (× 1,25) tenus par
  les mises en page ; mouvement réduit § 7.

## 11. Écarts par plateforme

| Sujet | libadwaita (GNOME HIG) | AppKit (Apple HIG) |
|---|---|---|
| Barre de titre | `AdwHeaderBar`, titre centré, fermer à droite | barre unifiée `NSToolbar`, feux à gauche, titre et sous-titre à gauche |
| Action principale | `suggested-action` à droite de l'en-tête | bouton par défaut (`↩`) ; à contour dans la barre d'outils ; en bas à droite des feuilles |
| Accent | accent système de l'utilisateur, non surchargé | `AccentColor` ambre profond, appliqué en « Multicolore » |
| Réglages | `AdwViewSwitcher` + `AdwPreferencesPage`, adaptatif | onglets de barre d'outils, taille fixe, `⌘,` |
| Listes | `boxed-list`, rangées 50 px, rayon 12 | sections groupées, rangées ~38 px, rayon 8, filets en retrait |
| Dialogues | `AdwAlertDialog`, boutons côte à côte, Annuler à gauche | `NSAlert` : icône, texte centré, boutons empilés, défaut en haut |
| Sous-tâche (test, import) | sous-page `AdwNavigationView` avec Retour | feuille attachée à la fenêtre |
| Erreur dans l'éditeur | `AdwBanner` | barre d'information maison sous la barre d'outils (pas d'équivalent natif) |
| Panneau de style | `AdwOverlaySplitView`, `F9` | inspecteur `NSSplitViewController`, `⌃⌘S` |
| Notifications | portail Notification ; GNOME Shell / Plasma dessinent | `UNUserNotificationCenter` ; actions au survol (« Options ») |
| Lien après envoi | GNOME : « Lien prêt — Copier le lien » ; KDE : copié | copié (`NSPasteboard`) |
| Miniature | GNOME : notification ; KDE, wlroots : layer-shell bas droit | `NSPanel` non activant, bas droit, tous les Spaces |
| Raccourcis globaux | portail GlobalShortcuts (GNOME confirme) ; wlroots : ligne `bindsym` | enregistreur dans Kropa ; `⌥` et `⌥⇧` seuls refusés |
| Notation des touches | `Ctrl+Maj+4` (FR), `Ctrl+Shift+4` (EN), touches dessinées | `⌃⇧4`, symboles sans séparateur |
| Raccourci de capture par défaut | `Ctrl+Maj+4` | `⌃⇧4` (évite `⌘⇧4` du système ; conflits à vérifier) |
| Pas de 10 px dans l'overlay | `Ctrl` | `⌥` |
| Permission de capture | portail Screenshot : dialogue GNOME une fois, refus mémorisé | TCC : Réglages Système, relance possible, rappel mensuel |
| Police | Adwaita Sans 11 pt, Adwaita Mono | SF Pro 13 px, SF Mono |
| Hauteur des contrôles | 34 px | 22–24 px |
| Focus | anneau 2 px, accent à 50 % | anneau système, couleur dérivée de l'accent |
| Accessibilité | AT-SPI via `GtkAccessible` | `NSAccessibility`, VoiceOver |
| Icônes standard | Adwaita symbolic (`edit-undo-symbolic`, `edit-copy-symbolic`, `document-save-symbolic`, `send-to-symbolic`, `open-menu-symbolic`, `sidebar-show-right-symbolic`, `dialog-password-symbolic`) | SF Symbols (`arrow.uturn.backward`, `doc.on.doc`, `square.and.arrow.down`, `square.and.arrow.up`, `sidebar.right`, `lock`) |
| Icônes d'outils | SVG symboliques recolorés | PDF gabarit (`…Template`) |

## 12. Icônes d'outils (D2.2)

Fichiers `docs/design/icons/kropa-tool-{arrow,rectangle,ellipse,line,text,pencil,highlighter,counter,blur,pixelate,mask}-symbolic.svg`.

- Grille 20 px, marge 2 px, **contours pleins** qui reproduisent un trait de 1,75 px (GTK ne recolore de façon fiable
  que les aplats), angles vifs, extrémités carrées, une couleur `#2e3436`.
- Métaphores : flèche à chevron d'équerre (rappel du logo), cadre, ellipse, ligne, T à empattements géométriques,
  crayon à pointe pleine, marqueur biseauté + trait de surlignage, cercle « 1 », points de taille croissante vers le
  centre (flou), mosaïque en X (pixellisation), texte caviardé (masque).
- Affichage à **20 px** (`-gtk-icon-size: 20px` ; taille de point 20 sous AppKit). À 16 px le trait tombe à 1,4 px :
  à éviter.
- Installation GTK : ressource `…/icons/scalable/actions/` ; macOS : export PDF gabarit au build.
- Les icônes standard (annuler, copier…) viennent du système (§ 11), pas de ce jeu.

## 13. Rédaction

Ton de `brand.md` (phrase courte, vouvoiement, pas de point d'exclamation, mêmes termes : capture, envoi,
destination, file d'attente, coffre). Les chaînes des maquettes servent de base à D2.4 (`.ftl`) ; espaces insécables
du français et formats de date à reprendre par `copywriter`.

## 14. Points ouverts

1. Accent : laisser l'accent système GNOME (identité discrète sous GNOME) plutôt que l'ambre ; à confirmer.
2. Texte secondaire : remplacer `.dim-label` / `secondaryLabelColor` par `k-muted` pour l'information (§ 4.3).
3. Cibles de 44 px limitées à nos surfaces (§ 10).
4. Overlay : `Entrée` sans sélection = écran entier ; `Tab` = écran suivant ; sélection limitée à un écran ; pas de
   10 px sur `Ctrl` / `⌥`.
5. Échec 401/403 : « Mettre à jour le jeton » plutôt que « Réessayer » (écart au tableau de `brand.md`).
6. Miniature macOS sans focus clavier : raccourci global « Ouvrir la dernière capture » à ajouter au lot 4.3 ?
7. Police embarquée pour le texte des annotations (Inter proposé).
8. Raccourci macOS par défaut `⌃⇧4` : conflits à vérifier sur le Mac de test.
