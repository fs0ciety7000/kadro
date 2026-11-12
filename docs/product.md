# Kropa — proposition de l'équipe Produit

> Proposition d'équipe du 11/11/2026. En cas de contradiction, docs/decisions.md prévaut.

> Statut : proposition, à valider. Source : `docs/brief.md`. Aucune décision technique n'est prise ici ;
> les points qui dépendent des équipes Plateformes, Architecture ou Envoi sont marqués et listés en fin de document.
> Légende : **[SRP]** = sous réserve plateforme (faisabilité à confirmer par l'équipe Plateformes).

## 1. Personas et tâches principales

| Persona | Contexte | Tâches principales | Ce qui le fait partir |
|---|---|---|---|
| **Léa, développeuse** | Fedora GNOME (Wayland), Zipline auto-hébergé | Capturer une zone, flécher un bug, coller un lien Markdown dans une PR ou un ticket | Plus de 3 touches entre capture et lien ; envoi vers un service qu'elle n'a pas choisi |
| **Marc, admin système** | Kubuntu KDE (Wayland) + serveurs, S3/MinIO et SFTP | Capturer un tableau de bord dans un script, envoyer, récupérer l'URL sur la sortie standard | Une CLI qui ne couvre pas tout, des codes de sortie flous, des secrets en clair |
| **Sophie, rédactrice de documentation** | macOS, guides utilisateur | Captures propres et homogènes, compteurs d'étapes, flou, habillage identique d'une capture à l'autre, retouche après coup | Rendu irrégulier, annotations figées une fois exportées |
| **Karim, support** | Ubuntu ou macOS, tickets clients | Capture rapide, masquer les données client, courte vidéo ou GIF de reproduction, lien collé dans la réponse | Oublier de flouter un e-mail ; vidéo trop lourde à envoyer |

Persona principal de la v1 : **Léa** (puis Marc). Leur parcours « zone → flèche → lien » est le cœur du produit ;
Sophie et Karim sont servis en partie en v1 (annotation, flou) et pleinement après (habillage, vidéo).

## 2. Parcours utilisateur clés (au clavier)

Raccourcis donnés à titre indicatif : tous sont configurables. Sur GNOME Wayland, le raccourci global peut
devoir être déclaré dans les réglages du bureau et appeler la CLI (voir §6).

### P1. Capture de zone → annotation → envoi → lien copié (Léa)

1. `Ctrl+Maj+4` (raccourci global) : l'écran se fige, voile sombre, réticule ; loupe au pixel et dimensions `W×H` près du curseur.
2. Sélection de la zone à la souris, **ou** au clavier : flèches pour déplacer le réticule, `Maj+flèches` pour agrandir, `Entrée` pour valider. `R` reprend la dernière zone. `Échap` annule à tout moment.
3. `E` (ou `Entrée` si « ouvrir l'éditeur » est le réglage par défaut) : ouverture de l'éditeur sur la zone.
4. Outils à une touche : `A` flèche, `R` cadre, `O` ellipse, `L` ligne, `T` texte, `P` crayon, `B` flou, `X` pixellisation, `M` masque plein, `N` compteur, `H` surligneur. `Ctrl+Z`/`Ctrl+Maj+Z`. `Tab` passe d'une annotation à l'autre, `Suppr` efface.
5. `Ctrl+U` : enregistrement local (toujours) puis envoi vers le profil par défaut ; `Ctrl+Maj+U` ouvre le choix du profil.
6. Notification « Lien copié (Markdown) » ; le format (URL/Markdown/HTML) suit le profil. L'éditeur se ferme.
7. Échec réseau : le fichier local est conservé, la notification le dit, la capture part en file d'attente (reprise plus tard ou `kropa queue retry`). Jamais de « lien copié » sans lien réel.

### P2. Capture rapide sans éditeur (Karim)

1. `Ctrl+Maj+3` : capture de l'écran actif (ou `Ctrl+Maj+4` puis `Entrée` direct si le réglage « après capture » est « actions rapides »).
2. La chaîne d'actions par défaut s'applique : enregistrer → copier l'image → (option) envoyer et copier le lien.
3. Miniature flottante en coin pendant 5 s (v1.x) : `Entrée` ouvre l'éditeur, `Ctrl+C` copie, `Ctrl+U` envoie, `Échap` ferme. En v1, une notification système avec actions remplit ce rôle.

### P3. Enregistrement vidéo ou GIF (Karim) — après v1

1. `Ctrl+Maj+5` : sélection de zone (comme P1), d'une fenêtre ou de l'écran ; `V` = MP4, `G` = GIF ; `M` active le micro, `S` l'audio système [SRP].
2. `Entrée` : compte à rebours 3 s, puis enregistrement ; indicateur discret avec durée.
3. `Ctrl+Maj+5` à nouveau (ou icône) : arrêt. `Ctrl+Maj+P` : pause/reprise.
4. Fenêtre de coupe : `[` et `]` fixent début et fin, `Espace` lit, `Entrée` exporte.
5. Suite identique à P1 : enregistrement local, envoi par morceaux si gros fichier, lien copié.

### P4. Ligne de commande dans un script (Marc)

```sh
url=$(kropa capture screen --output ~/rapports/ --name '{date}-{host}' --upload s3-prod --link-format url --no-clipboard)
kropa capture region --last --delay 3 --upload default    # reprise de la dernière zone
kropa upload ./schema.png --profile zipline               # envoi d'un fichier existant
kropa history list --search grafana --json
```

Exigences : sortie standard = résultat seul (chemin ou URL), messages sur la sortie d'erreur, `--json` partout,
codes de sortie documentés (0 succès, distincts pour annulation utilisateur, échec capture, échec envoi, config).
Toute action de l'interface a son équivalent CLI (principe du brief), y compris `kropa edit <fichier>`.
Limite à dire clairement : sous Wayland, une capture exige une session graphique et peut afficher une
autorisation du portail la première fois [SRP] ; pas de capture « sans écran » sur serveur.

### P5. Configuration d'une première destination (Léa)

1. Premier lancement : rien n'est envoyé, aucune requête réseau. Assistant : dossier de sauvegarde, langue, raccourcis. L'envoi est présenté comme optionnel.
2. Réglages → Destinations → `Ajouter` : choix du type (Zipline, S3, SFTP, Imgur, HTTP personnalisé, importer `.sxcu`).
3. Formulaire minimal (Zipline : URL + jeton). Le jeton va au trousseau ; s'il n'y en a pas, création ou saisie du mot de passe maître du fichier chiffré, avec explication.
4. `Tester` : envoi d'une image témoin de 1 px **après confirmation**, affichage de l'URL obtenue et du lien formaté ; suppression proposée si l'API le permet.
5. `Définir par défaut`, choix du format de lien. Équivalent CLI : `kropa dest add zipline --url … ` (jeton demandé sur l'entrée, jamais en argument), `kropa dest test`, `kropa dest import fichier.sxcu`.

## 3. Priorisation (MoSCoW) de toutes les fonctionnalités du brief

**M** = indispensable à la v1.0 · **S** = v1.0 si le temps le permet, sinon v1.x · **C** = après v1 · **W** = pas avant un besoin avéré.

### Capture
| Fonctionnalité | Prio | Justification |
|---|---|---|
| Zone | M | Parcours P1, l'usage le plus fréquent. |
| Plein écran / écran actif | M | Trivial une fois la capture en place ; base de la CLI. |
| Loupe au pixel + dimensions | M | Sans elle la sélection de zone est imprécise ; attendu par toutes les références. |
| Capture différée | M | Coût faible, indispensable pour les menus et survols. |
| Reprise de la dernière zone | S | Coût faible, gros gain pour Marc et la doc ; dépend seulement de l'état local. |
| Fenêtre « propre » (isolée, ombre, fond) | S [SRP] | Très demandée par Sophie, mais isolement de fenêtre et transparence varient fort selon Wayland/macOS. |
| Pipette | C | Utile mais secondaire ; la loupe affiche déjà la couleur. |
| Règle en pixels | C | Les dimensions de sélection couvrent 80 % du besoin. |
| OCR d'une zone | C | Dépendance lourde côté Linux (Tesseract) ; macOS a Vision. Valeur réelle mais hors cœur. |
| Détection de QR codes | C | Petite bibliothèque, mais usage de niche. |
| Capture défilante | W [SRP] | Très coûteuse et fragile (aucune API générique sous Wayland) ; à réévaluer après v1. |

### Annotation
| Fonctionnalité | Prio | Justification |
|---|---|---|
| Flèches, cadres, ellipses, lignes, texte, crayon | M | Le minimum d'un éditeur de capture. |
| Flou, pixellisation, masque plein | M | Sécurité : on envoie vers le web ; le masque plein est le seul sûr à 100 %. |
| Compteurs d'étapes | M | Coût faible, très utile à la doc et au support. |
| Surligneur | S | Coût faible (un cadre semi-transparent). |
| Projecteur (assombrir le reste) | C | Effet agréable, non essentiel. |
| Annotations rééditables après coup | S | Promesse forte pour Sophie ; le **format** doit être décidé dès v0.3 même si la réouverture arrive plus tard. |
| Habillage (fond, marge, ombre, préréglages) | C | Valeur esthétique, pas fonctionnelle ; vient avec la fenêtre propre. |
| Détection assistée des secrets | C | Différenciant mais coûteux (OCR requis) et risque de fausse sécurité ; à faire après l'OCR. |

### Vidéo et GIF
| Fonctionnalité | Prio | Justification |
|---|---|---|
| Enregistrement zone/écran en MP4 | C | Gros chantier (encodage, FFmpeg, licence, deux piles de capture) : premier lot après v1. |
| Export GIF | C | Même pile que MP4 ; utile au support. |
| Enregistrement d'une fenêtre | C [SRP] | Dépend du portail / de ScreenCaptureKit. |
| Pause / reprise | C | Livré avec la vidéo. |
| Coupe début/fin | C | Livré avec la vidéo ; sans coupe la vidéo est peu partageable. |
| Micro | C | Livré dans un second temps (pile audio). |
| Audio système | C [SRP] | PipeWire sous Linux ; macOS 13+ via ScreenCaptureKit à confirmer. |
| Affichage des clics et raccourcis | W [SRP] | Sous Wayland, lire les entrées globales est interdit sans privilèges ; macOS exige une autorisation d'accessibilité. |

### Après la capture
| Fonctionnalité | Prio | Justification |
|---|---|---|
| Enregistrement local | M | Principe « local d'abord » ; filet de sécurité de tout envoi. |
| Copie de l'image dans le presse-papiers | M | Implicite dans le brief, attendue partout (présence du presse-papiers à vérifier sous Wayland [SRP]). |
| Suppression des métadonnées avant envoi | M | Coût faible, promesse de confidentialité. |
| Chaîne d'actions | M (simple) / C (configurable) | v1 : cases à cocher fixes (enregistrer, copier, envoyer, ouvrir l'éditeur). Chaîne libre façon ShareX après. |
| Miniature flottante glisser-déposable | S [SRP] | Très appréciée (CleanShot), mais position et glisser-déposer fragiles sous Wayland ; notification en repli. |
| Historique avec recherche | S | v1 : liste CLI + dossier ; interface de recherche ensuite. |
| Épinglage au-dessus des fenêtres | W [SRP] | GNOME Wayland ne permet pas à une application de se mettre au premier plan d'elle-même. |

### Envoi
| Fonctionnalité | Prio | Justification |
|---|---|---|
| Destination HTTP personnalisée | M | Une seule implémentation couvre Zipline, Imgur et la plupart des hébergeurs. |
| Import `.sxcu` | M | Réutilise l'écosystème ShareX ; se ramène au moteur HTTP. |
| Zipline | M | Préréglage du moteur HTTP ; cible auto-hébergement prioritaire. |
| S3 et compatibles | M | Très demandé (MinIO, R2, B2) ; signature SigV4 à part du moteur HTTP. |
| SFTP | S | Demandé par les admins, mais pile SSH distincte (clés, hôtes connus). |
| Imgur | S | Préréglage HTTP ; question ouverte de la clé client (§7). |
| Plusieurs profils, un par défaut | M | Nécessaire dès deux destinations ; coût faible. |
| Copie du lien URL / Markdown / HTML | M | Le cœur du différenciant. |
| Secrets au trousseau + repli fichier chiffré | M | Contrainte du brief, préalable à tout envoi authentifié. |
| File d'attente avec reprise | S | v1 : reprise manuelle des échecs ; reprise automatique ensuite. |
| Envoi par morceaux | C | Utile surtout pour la vidéo ; arrive avec elle. |

### Transversal
| Fonctionnalité | Prio | Justification |
|---|---|---|
| CLI complète | M | Principe du brief ; testable, et repli des raccourcis sous Wayland. |
| Raccourcis globaux configurables | M [SRP] | Indispensables à l'usage ; repli : raccourci du bureau qui appelle la CLI. |
| Modèles de nom de fichier | M | Coût faible, utile dès la CLI. |
| Interface FR / EN | M | Contrainte du brief ; à poser dès le début (coût de reprise élevé après). |

**Bilan v1.0** : capture d'image complète au clavier, éditeur essentiel, envoi HTTP/sxcu/Zipline/S3, liens, secrets,
CLI, raccourcis, FR/EN, sur GNOME Wayland, KDE Wayland et macOS récent. **Pas de vidéo en v1.**

## 4. Options de découpage de la livraison

| Critère | A. Capture + annotation d'abord, envoi ensuite | B. Tranche verticale capture → envoi d'abord (GUI) | C. CLI / cœur d'abord, GUI ensuite |
|---|---|---|---|
| Premier livrable utile | Un Flameshot de plus (sans différenciant) | Le différenciant, mais via une GUI minimale à finir partout | Le différenciant, scriptable, dès v0.1 |
| Risques plateforme levés tôt | Capture oui, trousseau/presse-papiers/réseau tard | Tous, mais mêlés au chantier GUI | Capture, trousseau, presse-papiers, réseau, sans GUI |
| Coût de reprise | Fort : l'envoi impose ensuite file, secrets, notifications dans une GUI déjà figée | Moyen | Faible : la GUI appelle un cœur déjà testé |
| Usage réel par le développeur (dogfooding) | Oui, local seul | Oui | Oui, via raccourci du bureau → CLI |
| Faiblesse | Retarde ce qui distingue Kropa | Grosse marche initiale pour un développeur seul | Pas d'annotation avant v0.3 ; peu montrable au début |

Référence : ShareX bâtit tout sur l'enchaînement « tâches après capture → tâches après envoi » et des envoyeurs
personnalisés décrits par fichier ([guide ShareX](https://getsharex.com/blog/how-to-upload-screenshots-automatically/)) ;
c'est exactement ce qu'un cœur scriptable doit porter.

**Choix : C, conduit comme une tranche verticale** (C + B). v0.1 traverse toute la chaîne capture → fichier →
envoi → lien → presse-papiers, en CLI, sur les trois plateformes de référence. La GUI (sélection, puis éditeur,
puis flux) se pose ensuite sur ce cœur.
Raisons : (1) le différenciant et la promesse « local d'abord, secrets protégés » sont prouvés en premier ;
(2) les risques les plus incertains (portail de capture, presse-papiers Wayland, trousseau, signature ad hoc et
autorisation d'enregistrement d'écran sur macOS) tombent avant tout investissement GUI ; (3) la CLI est exigée de
toute façon et rend chaque étape testable ; (4) un développeur seul garde un produit utilisable à chaque jalon.

## 5. Plan de livraison

| Jalon | Contenu | Critère de fin |
|---|---|---|
| **v0.1 Cœur scriptable** | `kropa capture screen` et `kropa capture output`, enregistrement local, modèles de nom, envoi HTTP perso + `.sxcu` + préréglage Zipline, lien URL/MD/HTML copié, secrets au trousseau, FR/EN posés, journal sans données sensibles | Sur GNOME Wayland, KDE Wayland et macOS : `kropa capture screen --upload` donne un lien valide dans le presse-papiers ; un test prouve zéro requête réseau sans destination configurée |
| **v0.2 Sélection** | Superposition de sélection de zone avec loupe et dimensions, sélection au clavier, délai, dernière zone, copie d'image, profils, S3, repli fichier chiffré, `kropa dest add/test/import` | P4 et P5 (version CLI) réalisables de bout en bout ; P1 sans annotation réalisable |
| **v0.3 Éditeur** | Outils essentiels (formes, texte, crayon, flou, pixellisation, masque, compteurs, surligneur), annuler/rétablir, tout au clavier ; format d'annotation rééditable **décidé** et documenté | P1 complet réalisable sans souris sur les trois plateformes |
| **v0.4 Flux quotidien** | Résident (barre système / menu), raccourcis globaux ou repli documenté, réglages graphiques (destinations, raccourcis, chaîne d'actions simple), notifications avec actions, suppression des métadonnées, file d'attente avec reprise manuelle, SFTP | Le développeur utilise Kropa deux semaines comme seul outil de capture |
| **v1.0 Stable** | Paquets (dmg ad hoc + Homebrew tap, Flatpak et/ou deb/rpm/AppImage, PKGBUILD dans le dépôt faute d'AUR), docs dont page « signature ad hoc et ses conséquences », matrice de compatibilité honnête, Should restants si prêts (miniature, historique, rééditer, Imgur) | Tous les Must validés sur la matrice de référence ; aucun bogue bloquant ouvert ; chaque fonction dégradée a son repli documenté |
| **v1.1 Vidéo** | MP4 et GIF zone/écran, pause/reprise, coupe, micro, envoi par morceaux | P3 sans audio système réalisable sur les trois plateformes |
| **v1.2 Rendu soigné** | Fenêtre propre, habillage et préréglages, projecteur, historique avec recherche graphique, rééditer | Un guide de Sophie réalisable avec rendu homogène |
| **v1.3 Lecture d'image** | OCR, QR, détection assistée des secrets, pipette, règle | OCR FR/EN utilisable hors ligne sur les deux OS |
| **Plus tard** | Audio système, fenêtre en vidéo, chaîne d'actions libre, capture défilante, épinglage, affichage des clics | Chacun après étude Plateformes concluante |

## 6. Risques produit

| Risque | Effet | Repli / mesure |
|---|---|---|
| **Raccourcis globaux sous Wayland** [SRP] | Portail GlobalShortcuts inégal selon GNOME/KDE/wlroots ; Flameshot classe encore GNOME et Plasma Wayland « expérimental » ([aide Wayland Flameshot](https://flameshot.org/docs/guide/wayland-help/)) | Raccourci du bureau qui appelle `kropa capture …`, documenté par bureau ; assistant qui le propose |
| **Capture via portail** [SRP] | Dialogue d'autorisation, pas de gel d'écran natif, multi-écrans à vérifier | Capturer d'abord l'écran entier puis sélectionner sur l'image figée ; matrice publiée |
| **Presse-papiers Wayland** [SRP] | Le contenu disparaît si le processus se termine | Résident (v0.4) ou processus détaché qui sert le presse-papiers ; à trancher avec Architecture |
| **macOS sans signature Apple** | Avertissement Gatekeeper ; l'autorisation « Enregistrement de l'écran » risque d'être redemandée à chaque mise à jour, la signature ad hoc changeant à chaque build [SRP] | Page de doc honnête ; installation Homebrew ; tester tôt (v0.1) |
| **Épinglage, miniature flottante** [SRP] | GNOME Wayland interdit placement et premier plan imposés | Notification avec actions ; épinglage hors v1 |
| **Capture défilante** [SRP] | Pas d'API générique ; résultat souvent faux | Hors v1 ; ne jamais livrer un assemblage approximatif présenté comme fiable |
| **Audio système macOS** [SRP] | Dépend de ScreenCaptureKit et de la version de macOS | Micro seul, audio système annoncé par version |
| **Affichage des clics/raccourcis** [SRP] | Impossible sous Wayland sans privilèges | Non prévu ; à rouvrir si un portail l'offre |
| **Fausse sécurité du flou** | Un flou faible se reconstitue ; la détection de secrets peut manquer un jeton | Masque plein recommandé pour les secrets ; flou fort par défaut ; libellé « assistée », jamais « garantie » |
| **Dérive de périmètre** | Développeur seul : la liste du brief représente plusieurs années | Règle : pas de fonctionnalité C avant v1.0 ; chaque jalon doit rester utilisable |
| **Licence et FFmpeg** | Choix GPL/LGPL contraint par l'édition FFmpeg retenue | Décision avant v1.1 ; aucun code vidéo avant |
| **`.sxcu` non fiable** | Un fichier importé peut envoyer des données ailleurs | Aperçu de l'hôte et des champs à l'import, confirmation explicite |

Observations sur les références : CleanShot X montre après chaque capture une vignette en coin d'où l'on copie,
annote, enregistre ou envoie et récupère un lien ([fonctionnalités CleanShot X](https://cleanshot.com/features)) — c'est
le modèle de la miniature v1.x. macshot, libre et natif macOS, prouve qu'un éditeur à outils à une touche, flou et
pixellisation tient dans un projet communautaire ([dépôt macshot](https://github.com/sw33tLie/macshot)). Spectacle n'a
l'enregistrement que depuis Plasma 6 et pas sous X11 ([discussion KDE](https://discuss.kde.org/t/redesign-spectacle/15127),
[Plasma 6.6](https://www.omgubuntu.co.uk/2026/02/kde-plasma-6-6-released)) : la vidéo Linux est réelle mais jeune,
ce qui conforte son report après v1.

## 7. Questions ouvertes et dépendances avec les autres équipes

**Plateformes**
1. Matrice de faisabilité par bureau (GNOME, KDE, Sway/Hyprland, X11, macOS 13/14/15) pour : capture d'écran et de zone, fenêtre isolée avec transparence, raccourcis globaux, presse-papiers persistant, fenêtre au premier plan (épinglage, miniature), audio système, lecture des entrées.
2. Versions minimales visées (GNOME, Plasma, macOS) : elles fixent la promesse de la v1.
3. Conséquence exacte de la signature ad hoc sur l'autorisation d'enregistrement d'écran macOS à chaque mise à jour.
4. Sélection de zone : superposition propre à Kropa sur image figée, ou sélecteur du portail ? Impact direct sur la loupe et la sélection au clavier.

**Architecture**
5. Un processus résident est-il requis (presse-papiers Wayland, raccourcis, file d'attente) ? Si oui, la CLI lui parle-t-elle ou fonctionne-t-elle seule ?
6. Boîte à outils GUI (rendu, accessibilité, i18n, saisie clavier) compatible avec « natif et léger ».
7. Format d'annotation rééditable : fichier compagnon, métadonnées embarquées, ou projet séparé ? Doit être tranché avant v0.3.
8. Licence du projet, liée au choix FFmpeg (édition LGPL en liaison dynamique ou GPL) — avant v1.1.

**Envoi et sécurité**
9. Imgur exige une clé client d'application : en livrer une (dépendance à un compte tiers et requête « non configurée » ?) ou exiger celle de l'utilisateur ? Proposition Produit : celle de l'utilisateur.
10. Périmètre exact du format `.sxcu` supporté (versions, syntaxe d'extraction d'URL) et ce qui est refusé.
11. Repli fichier chiffré : comportement quand le mot de passe maître est demandé depuis un script (CLI non interactive).
12. Destination S3 : URL publique ou URL présignée à durée limitée ? Les deux posent des questions de confidentialité différentes.

**Distribution**
13. Format(s) Linux retenu(s) sans AUR : Flatpak (portails natifs, mais bac à sable) vs paquets natifs vs AppImage.

**Désaccords possibles à signaler** : si Plateformes juge les raccourcis globaux Wayland trop fragiles pour être un
Must, Produit demande de garder le Must avec repli CLI documenté plutôt que de le déclasser ; si Architecture
préfère un démarrage GUI, le désaccord sur le découpage (§4) doit être tranché par l'utilisateur.
