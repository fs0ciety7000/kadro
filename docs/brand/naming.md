# Nouveau nom — propositions

> Équipe Direction artistique, 11/11/2026. Remplace « Kadro » (éditeur vidéo libre homonyme `kadro-studio/kadro`,
> paquet npm `kadro` pris). Identité retenue inchangée : **A « Repère »** (deux équerres opposées, poignée ambre,
> graphite ; Space Grotesk + JetBrains Mono). Produit : capture d'image, annotation, masquage, envoi, lien copié.

**Ce n'est pas une vérification juridique.** Les contrôles ci-dessous portent sur les registres de paquets, les
dépôts de code et une recherche web rapide. Avant toute publication : recherche de marques (EUIPO, INPI, USPTO, WIPO
Global Brand Database) en classes 9 et 42, et vérification des noms de domaine (non vérifiés ici).

## 1. Méthode

1. **Critères** : 4 à 7 lettres ; se tape vite (`nom capture area`) ; se prononce de façon stable en français et en
   anglais ; aucune connotation gênante en FR, EN, ES, DE, IT, PT ; évoque cadre, capture, repère, découpe ou
   partage ; s'accorde avec les équerres et la poignée ; pas de proximité phonétique avec un logiciel de capture.
2. **Génération** (43 candidats), puis premier tri automatique (crates.io, npm, PyPI, Homebrew, Repology) et tri
   linguistique.
3. **Contrôle approfondi des finalistes** : crates.io, npm, PyPI, Homebrew formule et cask, Flathub (API de
   recherche), GitHub (dépôts au nom exact), Debian (noms de paquets, toutes versions), **fichiers nommés exactement
   ainsi dans les paquets Debian stable et Ubuntu 24.04** (conflit de binaire), Arch (dépôts officiels), Fedora
   (mdapi, rawhide), Repology (≈ 300 dépôts : Alpine, nixpkgs, openSUSE, MacPorts…), Wiktionary (sens dans les
   autres langues), recherche web de produits homonymes.

### Candidats écartés (et pourquoi)

| Raison | Candidats |
|---|---|
| Confusion avec un logiciel de capture ou de création visuelle | **kapto, kaptu, kapti** (homophones de *Capto*, logiciel macOS de capture et d'enregistrement d'écran de Global Delight) ; **ekran, ekrano** (*Ekran System*, devenu Syteca, enregistre les sessions écran) ; **bezel** (cask Homebrew pris) ; **kadr, kader, kadra, cadra** (trop proches de l'ancien nom et de l'éditeur vidéo) |
| Connotation gênante | **corna** (geste des cornes, cocu, en italien) ; **bildo** (se lit comme un mot vulgaire en anglais) ; **jalon** (*jalar* a des sens argotiques en espagnol d'Amérique) ; **liser** (« loser ») ; **mira**/**mire** (*mire* = bourbier en anglais) |
| Marque forte déjà établie | **mitre** (MITRE, sécurité informatique) ; **marq** (Garmin MARQ) ; **framo** (Framo, industrie) ; **trimo** (Trimo, bâtiment) |
| Pris sur plusieurs registres | kerf, ansa, margo, balise, canto, snipr, vizor, piksel, areo, kapti |
| Faible évocation ou prononciation instable | quadr, kvadro, encart, onglet, tiret, equer, pinne, amer, orla, orlo |

## 2. Les 6 finalistes

Disponibilités vérifiées le 11/11/2026 (« libre » = aucun paquet de ce nom au moment du contrôle).

| Nom | Sens, lien au logo | Prononciation | crates.io | npm | PyPI | Homebrew (formule / cask) | Flathub | Distributions et binaire | GitHub (nom exact) | Risques |
|---|---|---|---|---|---|---|---|---|---|---|
| **orle** | Héraldique (FR et EN) : bordure qui court **à l'intérieur** de l'écu sans en toucher le bord, soit la sélection dans l'écran. Ancien français *orle* : bordure. ES et PT *orlar* : border. PL et CS : « d'aigle », « aiglon ». | FR [ɔʁl] « orl » ; EN /ɔːl/ « orl » | libre | **pris** (encodeur RLE, abandonné depuis 2022) | libre | libre / libre | rien | aucun paquet (Debian, Ubuntu, Arch, Fedora, Repology) ; aucun fichier `orle` dans Debian stable ni Ubuntu 24.04 | aucun | Homophone de « ORL » en français (à l'oral, et en capitales). Mot rare : il faut raconter le sens. Bruit de recherche (Orleans, Orlebar). |
| **kropa** | Évoque *crop* (recadrer) et les repères de coupe d'imprimerie, que sont précisément les équerres du logo. PL : augmentatif de *kropka*, « point ». Village slovène. | FR « kro-pa » ; EN /ˈkroʊpə/ | libre | libre | libre | libre / libre | rien | aucun paquet ; aucun fichier `kropa` | 6 petits projets (diagnostic agricole, jeu Godot), aucun dans l'image | Sens inventé pour un francophone. À l'écrit, peut passer pour une faute de *crop*. |
| **ramka** | « Petit cadre » en polonais et en russe (diminutif de *rama*) : même famille que *kadr*. | FR « ram-ka » ; EN /ˈræmkə/ | libre | libre | **pris** (micro-cadriciel web, 0.1.2) | libre / libre | rien | aucun paquet ; aucun fichier `ramka` | **`arturbien/ramka`** (131 ★, actif) : visionneuse d'images React | Proximité de domaine avec la visionneuse React. Argot polonais : « paquet de cigarettes » (anodin). Sens opaque hors pays slaves. |
| **cadrat** | Typographie : bloc de plomb blanc qui règle les espaces (d'où *cadratin*). Rectangle, précision. | FR [ka.dʁa] ; EN incertain, probablement « KAD-rat » | libre | libre | libre | libre / libre | rien | aucun paquet ; aucun fichier `cadrat` | aucun | En anglais, se décompose en *cad* (goujat) + *rat*. Deux prononciations concurrentes. Proche de *cadran*, *quadrat*. |
| **reper** | « Repère » sans accent ; PL : borne géodésique ; RO : repère. Lien direct avec le nom de l'identité. | FR « ré-pèr » ou « reu-pèr » ; EN « REP-er » ou « REE-per » | libre | libre | libre | libre / libre | rien | aucun paquet ; aucun fichier `reper` | 8 petits projets (dont outil de génomique), aucun dans l'image | Se lit *reaper* en anglais (la mort ; **REAPER**, logiciel audio et vidéo connu). Albanais : « rappeur ». Ressemble à une faute de *repère* ou de `repr`. |
| **vinkel** | « Angle » en danois, norvégien, suédois (*vinkelhake* : équerre). | FR « vin-kel » ; EN « VIN-kel » | libre | **pris** (composants Angular, abandonné depuis 2022) | libre | libre / libre | rien | aucun paquet ; aucun fichier `vinkel` | 3 dépôts vides | Opaque en FR et EN. NL *winkel* : magasin. 6 lettres, frappe moins fluide. |

Notes : Flathub ne renvoie aucune application à ces noms (seulement des correspondances floues sans rapport). Fedora
et Arch : contrôle du nom de paquet seulement, pas du contenu des fichiers. npm n'est pas un canal de distribution
pour un outil Rust : y être « pris » gêne peu (repli possible : `@orle/cli`).

## 3. Recommandation

**1er — `orle`.** C'est le seul candidat dont le sens recouvre exactement le geste du produit et le logo : une
bordure tracée à l'intérieur d'un cadre, sans toucher le bord, ce que fait la sélection de zone et ce que dessinent
les deux équerres. Le mot existe tel quel en français et en anglais (même orthographe, même sens), et l'espagnol et
le portugais l'entendent comme « border » : peu de marques peuvent en dire autant. 4 lettres qui alternent les
mains sur QWERTY et AZERTY (`orle capture area`), aucune connotation gênante trouvée dans les six langues, et les
canaux qui comptent pour un outil Rust (crate, Homebrew, Flathub, Debian, Fedora, Arch) sont libres, sans conflit de
binaire. Risques acceptables : l'homophonie « ORL » (on écrit toujours le nom en bas de casse) et un mot rare
(une ligne dans le README suffit : « orle, n. m. : bordure intérieure de l'écu »).

**2e — `kropa`.** Libre partout, y compris npm. Il parle immédiatement aux anglophones (*crop*) et rejoint les traits
de coupe du logo. Il manque une histoire pour les francophones, et il peut passer pour une faute de frappe.

**3e — `ramka`.** Sens exact (« cadre »), dans la lignée de *kadr* ; prononciation stable. Mais PyPI est pris et une
visionneuse d'images React active porte ce nom : risque de confusion dans les recherches liées à l'image.

`cadrat` (risque de lecture en anglais), `reper` (*reaper*, REAPER) et `vinkel` (opaque) restent en réserve.

### Ce qui change dans le logo si `orle` est retenu (sans redessiner pour l'instant)

- **Symbole, palette, typographies, ton, icônes** : inchangés. Le concept garde son nom, « Repère ».
- **Mot-symbole** : « orle » en bas de casse, même dessin monolinéaire, terminaisons coupées net, même grille que
  les équerres. 4 lettres au lieu de 5 : le logo complet se raccourcit d'environ un cinquième ; refaire le test de
  taille minimale (aujourd'hui 80 px, probablement moins). Zone de protection inchangée (largeur de la poignée).
- **Glyphes** : « o » et « r » repris tels quels de « kadro » ; « l » repris du fût du « d » (seule ascendante du
  mot : l'aligner sur le haut de l'équerre du symbole) ; « e » à dessiner (barre horizontale coupée net, ouverture
  calée sur la grille). « k », « a », « d » disparaissent.
- **Règle d'usage** : jamais « ORLE » en capitales (lu « ORL » en français) ; « Orle » en début de phrase seulement.
- **Identifiants à mettre à jour plus tard** : commande et crate `orle`, identifiant d'application
  `io.github.<compte>.Orle`, dossier `~/Images/Orle`, exemples de ton (`orle capture area`), `preview.html`.

**À faire avant d'annoncer le nom** : réserver le crate `orle` et le dépôt GitHub ; recherche de marques en
classes 9 et 42 ; vérifier les domaines (`orle.app`, `orle.dev`…), non contrôlés ici.

## Sources

Registres interrogés directement : `crates.io/api/v1/crates/<nom>`, `registry.npmjs.org/<nom>`,
`pypi.org/pypi/<nom>/json`, `formulae.brew.sh/api/{formula,cask}/<nom>.json`, `flathub.org/api/v2/search`,
`repology.org/api/v1/project/<nom>`, `packages.debian.org` (noms et contenus), `packages.ubuntu.com` (contenus,
noble), `archlinux.org/packages/search/json`, `mdapi.fedoraproject.org/rawhide/pkg/<nom>`, API de recherche GitHub.

- Sens des mots : [orle](https://en.wiktionary.org/wiki/orle) · [kropa](https://en.wiktionary.org/wiki/kropa) ·
  [ramka](https://en.wiktionary.org/wiki/ramka) · [cadrat](https://en.wiktionary.org/wiki/cadrat) ·
  [reper](https://en.wiktionary.org/wiki/reper) · [vinkel](https://en.wiktionary.org/wiki/vinkel)
- Capto (capture d'écran macOS) : [Global Delight](https://www.globaldelight.com/capto/capto-screen-recording/index.php) ·
  [Mac App Store](https://apps.apple.com/us/app/capto-screen-capture-recorder/id1078184147?mt=12)
- Ekran System / Syteca : [Software Advice](https://www.softwareadvice.com/privileged-access-management/ekran-system-profile/vs/teramind/)
- Visionneuse React `ramka` : [github.com/arturbien/ramka](https://github.com/arturbien/ramka)
- REAPER (proximité de `reper`) : [worddisk.com/wiki/REAPER](https://worddisk.com/wiki/REAPER)
