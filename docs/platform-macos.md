# Kropa — plateforme macOS

> Proposition d'équipe du 11/11/2026. En cas de contradiction, docs/decisions.md prévaut.

> Équipe Plateforme macOS. État vérifié en ligne le 11 novembre 2026. Versions en circulation : macOS 26 « Tahoe »
> et macOS 27 « Golden Gate », sorti le 14 septembre 2026 pour Apple silicon seulement ([MacRumors][mr27], [9to5Mac][9to527]).
> « Non vérifié » signale une affirmation sans source primaire. Sinon, la source est donnée.

## 0. Ce qu'il faut retenir

- Toutes les fonctions du brief sont **réalisables** sur macOS 15+. Quatre sont **dégradées** : capture défilante,
  rappel périodique de la permission d'enregistrement d'écran, raccourcis Option seule, affichage des touches (qui exige
  une permission de plus). Aucune n'est **impossible**.
- **Le vrai coût de l'absence de signature Apple, c'est la distribution, pas l'API** : le cask Homebrew officiel
  est fermé, chaque ouverture d'une version téléchargée passe par « Ouvrir quand même », et, avec une signature
  *ad hoc*, **les permissions TCC et l'accès au Keychain sont redemandés à chaque mise à jour**.
- Recommandation centrale : signer chaque version avec un **certificat auto-signé stable** du projet, plutôt
  qu'en ad hoc. Gatekeeper reste bloquant, mais les permissions survivent aux mises à jour (§4.3).
- Version minimale recommandée : **macOS 15 Sequoia** (§3).

## 1. Matrice de faisabilité

Permissions TCC : **ER** = Enregistrement de l'écran et de l'audio système, **AX** = Accessibilité,
**SE** = Surveillance de l'entrée, **Mic** = Microphone. « — » = aucune permission.

| Fonctionnalité | Statut | macOS min. | Permission | Note |
|---|---|---|---|---|
| Capture zone / plein écran / écran actif | possible | 14 (`SCScreenshotManager`) | ER | `captureImage(contentFilter:configuration:)` + `sourceRect`. Écran actif : écran sous le pointeur (`NSEvent.mouseLocation`, sans permission). `captureImage(in:)` existe à partir de 15.2 ([doc][sck-inrect]). `CGWindowListCreateImage` n'est plus utilisable : il renvoie une image grise sur macOS 27 bêta 4+ ([forum][f849969]). |
| Loupe au pixel + overlay de sélection + dimensions | possible | 14 | ER | Capture figée de chaque écran affichée dans une fenêtre sans bord au-dessus de tout, une par écran. La loupe lit les pixels de l'image figée. Méthode courante des outils du genre (non vérifié pour chacun). |
| Fenêtre « propre » (isolée, ombre, fond transparent) | possible | 14 | ER | Filtre `desktopIndependentWindow` : image de la fenêtre seule, avec transparence. Ombre système réglable par `ignoreShadowsSingleWindow` (14.0, [doc][sck-shadow]). Pour garder la parité avec Linux, mieux vaut dessiner nous-mêmes l'ombre et le fond. La liste des fenêtres (`SCShareableContent`) demande ER. |
| Capture différée (compte à rebours) | possible | 14 | ER | Minuterie côté application. |
| Capture défilante | **dégradé** | 14 | ER (+ AX si défilement auto) | Aucune API système. L'utilisateur fait défiler pendant qu'un `SCStream` capture, puis on assemble par recalage d'images. macshot assemble avec Vision ([macshot][macshot]). Défilement automatique : événements synthétiques `CGEventPost`, donc AX. Échecs attendus sur les en-têtes fixes et le contenu animé. |
| Reprise de la dernière zone | possible | 14 | ER | Coordonnées mémorisées par écran. Attention aux changements de résolution ou de disposition des écrans. |
| Pipette | possible | 10.15 | — / ER | Depuis l'image figée (ER), ou `NSColorSampler` du système, sans permission ([doc][nscs]). |
| Règle en pixels | possible | 14 | ER | Dessinée sur l'image figée. Points ou pixels : gérer le facteur Retina (`backingScaleFactor`). |
| OCR d'une zone (Vision) | possible | 10.15 | — (sur l'image déjà capturée) | `VNRecognizeTextRequest` ([doc][vn-ocr]), traitement local, français et anglais pris en charge (liste exacte des langues non vérifiée). Les API Vision récentes réservées à Swift ne sont pas accessibles depuis objc2 (§5). |
| Détection de QR codes | possible | 10.13 | — | `VNDetectBarcodesRequest` ([doc][vn-bc]). |
| Enregistrement vidéo MP4 (zone / fenêtre / écran) | possible | 14 (15 conseillé) | ER | `SCStream` + `AVAssetWriter` (H.264/HEVC matériel), ou `SCRecordingOutput` (15.0, [doc][sck-rec]). La pause et la reprise sont à notre charge (horodatages). Le GIF est produit à partir des images capturées. |
| Audio système | possible | 13 | ER | `capturesAudio` (13.0, [doc][sck-audio]) : couvert par la même permission. Alternative : process tap Core Audio (14.2, [doc][ca-tap]), permission distincte (non vérifié). |
| Microphone | possible | 15 (dans `SCStream`) | Mic | `captureMicrophone` (15.0, [doc][sck-mic]) : une seule horloge pour l'image et le son. Sur 14, il faut un `AVCaptureSession` à part, à synchroniser soi-même. Exige `NSMicrophoneUsageDescription` dans l'Info.plist, donc un `.app` (non vérifié pour un binaire nu). |
| Affichage des clics | possible | 15 | ER | `showMouseClicks` (15.0, [doc][sck-clicks]) : le système dessine les clics dans le flux, sans permission de plus. Pour un style à nous : moniteur global `NSEvent` (souris, AX selon les cas, non vérifié). |
| Affichage des touches / raccourcis | **dégradé** | 10.15 | **SE** | Tap d'événements clavier en écoute seule (`CGEventTap` listen-only), soumis à Surveillance de l'entrée. Une permission de plus, à demander seulement si l'option est activée. Avec une signature ad hoc, elle est perdue à chaque mise à jour (§4.2). |
| Miniature flottante | possible | — | — | `NSPanel` non activant, coin de l'écran, `canJoinAllSpaces` + `fullScreenAuxiliary` ([doc][fsaux]). |
| Glisser-déposer sortant | possible | — | — | `NSDraggingSource` + `NSPasteboardItem` (fichier PNG + URL). |
| Épinglage au-dessus des fenêtres | possible | — | — | `NSWindow.level = .floating`, et passage au-dessus des apps en plein écran avec `fullScreenAuxiliary`. À exclure de la capture (`SCContentFilter` `excludingWindows`). |
| Raccourcis globaux | **dégradé** (partiel) | — | — | `RegisterEventHotKey` (Carbon), sans permission (crate `global-hotkey`). **Depuis macOS 15, les combinaisons Option seule ou Option+Maj sont refusées** (erreur -9868), volontairement ([forum][f763878], [Shottr][shottr-seq], [Eternal Storms][es-opt]). Un retour en 15.2 est annoncé, mais les sources se contredisent : non vérifié. |
| Presse-papiers | possible | — | — | `NSPasteboard` (image + texte du lien), crate `arboard` 3.7.0. |
| Keychain | possible, **dégradé sans signature stable** | — | invite Keychain | Keychain à fichier (ACL liées à la signature). Le Keychain « data protection » demande des entitlements autorisés par un profil de provisionnement ([TN3137][tn3137]), donc pas accessible sans compte. Voir §4.4. |
| Rappel périodique de la permission ER | **dégradé** (subi) | 15+ | ER | macOS 15 redemande périodiquement de confirmer l'accès, environ une fois par mois ([TidBITS][tidbits-m], [MacRumors 15.1][mr151]). Aucune exemption pour une app ordinaire : `persistent-content-capture` est réservé aux apps de type VNC, sur demande à Apple ([M. Tsai][mjt]). Toujours présent sur 26 et 27 : **non vérifié** (pas de note de version trouvée). Indépendant de la signature. |

### Tableau des permissions par fonction

| Permission | Pour | Quand la demander |
|---|---|---|
| ER | toute capture, la liste des fenêtres, la vidéo et l'audio système | au premier usage (`CGRequestScreenCaptureAccess`) ; état lu par `CGPreflightScreenCaptureAccess` ([forum][f839069]) |
| Mic | micro dans les vidéos | seulement si l'option est cochée |
| SE | affichage des touches | seulement si l'option est cochée |
| AX | défilement automatique de la capture défilante | seulement si cette option est activée |

Note macOS 27 : une catégorie « Bureau à distance » distincte apparaît dans Réglages, et aucune API ne lit son état
([forum][f841530]). Elle ne devrait concerner que les apps de type VNC (non vérifié pour Kropa).

## 2. Replis pour les cas dégradés

- **Capture défilante** : v1 en défilement **manuel**. L'utilisateur fait défiler et Kropa assemble. Le défilement
  automatique est une option qui demande AX. Si l'assemblage échoue, on le dit (« assemblage incertain »), on propose
  l'image brute et on n'invente jamais de raccord.
- **Rappel mensuel ER** : impossible à supprimer. Expliquer dans l'accueil pourquoi la boîte réapparaît. Ne pas
  conseiller d'outils de contournement non officiels (Amnesia, réglages en Terminal ([Dropshare][dropshare])).
- **Sélecteur système `SCContentSharingPicker`** (14.0, [doc][sck-picker]) : il évite la permission ER, mais impose
  une interface système à chaque capture et ne permet pas de choisir une fenêtre dans le sélecteur initial sur macOS 15
  ([Mozilla][moz-picker]). C'est un **repli** quand ER est refusée (mode « capture par sélecteur »), pas le chemin
  normal : il est incompatible avec « tout au clavier » et avec la loupe.
- **Raccourcis Option seule** : refuser la combinaison à la saisie avec un message clair, et proposer par défaut des
  raccourcis avec ⌘ ou ⌃ (Shottr fait de même, [Shottr][shottr-seq]).
- **Affichage des touches** : désactivé par défaut. Quand on l'active, Kropa explique la permission SE et ce qu'elle
  permet.
- **Keychain sans signature stable** : voir §4.4. En dernier recours, le fichier chiffré par mot de passe maître déjà
  prévu par le brief.

## 3. Version minimale de macOS

| Critère | macOS 14 Sonoma | **macOS 15 Sequoia** | macOS 26 Tahoe |
|---|---|---|---|
| `SCScreenshotManager`, sélecteur, ombre de fenêtre | oui | oui | oui |
| Micro dans `SCStream` (une seule horloge) | non : AVCapture séparé | **oui** | oui |
| Clics dessinés par le système | non : à refaire | **oui** | oui |
| `SCRecordingOutput` (MP4 direct) | non | oui | oui |
| Captures HDR / `SCScreenshotConfiguration` | non | non | oui (26.0, [doc][sck-cfg]) |
| Raccourcis Option seule | oui | refusés | refusés |
| Rappel mensuel ER | non | oui | oui (non vérifié) |
| Macs Intel | oui | oui | dernière version Intel (27 = Apple silicon seulement, [MacRumors][mr27]) |
| Coût pour un développeur seul | deux chemins audio et clics à maintenir | un seul chemin | parc exclu plus large, gain faible |

**Choix : macOS 15 Sequoia.** On gagne un seul pipeline vidéo et audio (micro et clics fournis par SCK), ce qui
compte pour un développeur seul. Le rappel mensuel et le blocage des raccourcis Option touchent de toute façon 15, 26
et 27. Les fonctions propres à 26 (HDR) seront ajoutées au cas par cas, détectées à l'exécution. Architectures :
`aarch64` + `x86_64` (binaire universel) tant que Tahoe tourne sur Intel. La part du parc par version n'est pas
vérifiée.

## 4. Conséquences de l'absence de signature Apple

### 4.1 Première ouverture (Gatekeeper)

- Une app téléchargée porte l'attribut de quarantaine et, faute de notarisation, son lancement est bloqué. Depuis
  macOS 15, le **clic droit > Ouvrir ne contourne plus Gatekeeper** ([AppleInsider][ai-seq]). Seul chemin officiel :
  Réglages > Confidentialité et sécurité > **Ouvrir quand même**, puis confirmation et **mot de passe administrateur**
  ([Apple][apple-open], [Eclectic Light][el-nonot]). Une app ad hoc passe par ce chemin et fonctionne ensuite
  ([Eclectic Light][el-nonot]). Le bouton n'apparaîtrait que pour les apps signées (ad hoc inclus) : non vérifié.
- Autre voie : `xattr -d com.apple.quarantine Kropa.app`. Elle est à documenter honnêtement, comme une décision de
  l'utilisateur qui saute les contrôles de premier lancement ([Eclectic Light][el-nonot]).
- Apple prévient que passer outre est « le moyen le plus courant d'infecter un Mac » ([Apple][apple-safe]). C'est
  l'image que donnera Kropa, à assumer dans la documentation.
- Chaque nouvelle version téléchargée est à nouveau en quarantaine. Il faut donc probablement refaire « Ouvrir quand
  même » à chaque mise à jour (non vérifié : à tester sur 15, 26 et 27).
- Un binaire **compilé sur la machine** (`cargo install`, formule Homebrew compilée depuis les sources) n'est pas en
  quarantaine. Gatekeeper ne bloque donc pas son lancement ([Eclectic Light][el-nonot]). L'éditeur de liens signe en
  ad hoc automatiquement sur Apple silicon (non vérifié ici, comportement connu).

### 4.2 Permissions TCC et mises à jour

- TCC enregistre la **Designated Requirement (DR)** de l'app et la vérifie à chaque accès. Avec une signature ad hoc,
  la DR « est liée à cette version précise du code » : macOS ne peut pas suivre l'app d'une version à l'autre
  ([TN3127][tn3127]). Apple l'a confirmé pour ScreenCaptureKit : avec une signature ad hoc, chaque build est traité
  comme une nouvelle app ([forum, Quinn][f819406]).
- **Conséquence en signature ad hoc** : à **chaque mise à jour**, l'utilisateur doit redonner ER (et Mic, SE, AX
  selon les options). Les anciennes entrées s'accumulent ou restent cochées sans effet. macshot documente ce symptôme
  (« activé mais la capture échoue », retirer puis rajouter l'app) au changement de sa signature ([macshot][macshot]).
- Un binaire nu, hors `.app`, n'apparaît plus dans la liste ER sur 26.1. Apple y voit un bug ([forum][f807898]). Il
  faut donc **toujours livrer un `.app`**, y compris pour la CLI : le binaire `kropa` vit dans le bundle et un lien
  symbolique le met dans le `PATH`.

### 4.3 Atténuation recommandée : certificat auto-signé stable

- Signer avec un certificat de signature de code **auto-signé** (créé une fois, gardé par le mainteneur) donne une DR
  de la forme `identifier "…" and certificate leaf = H"…"`, stable d'une version à l'autre. Les autorisations TCC sont
  alors conservées. C'est la méthode documentée par yabai et skhd.zig ([yabai][yabai], [skhd.zig][skhd]), expliquée
  par [nickboy][nickboy]. Elle se déduit du mécanisme décrit dans [TN3127][tn3127]. **À vérifier sur 15, 26 et 27**
  (sources non Apple).
- Ce que cela **ne change pas** : Gatekeeper (pas de notarisation, donc toujours « Ouvrir quand même »), le rappel
  mensuel ER, le Keychain « data protection ».
- Risques : si la clé privée fuit, n'importe qui peut signer un binaire qui hérite des permissions accordées à Kropa.
  Changer de certificat fait perdre toutes les permissions à tous les utilisateurs ([skhd.zig][skhd]). La clé doit
  donc être un secret de CI.
- Pour une compilation locale (formule ou `cargo install`), on peut proposer un script qui crée un certificat
  personnel et signe le `.app`, comme yabai.

### 4.4 Keychain

- Le Keychain « data protection » est réservé aux entitlements autorisés par un profil de provisionnement
  ([TN3137][tn3137]), donc fermé sans compte (le code d'erreur exact sans profil n'est pas vérifié). Kropa doit
  utiliser le **Keychain à fichier** (login). Son contrôle d'accès est une ACL qui désigne l'app par sa DR
  ([TN3137][tn3137], [TN3127][tn3127]).
- En ad hoc : après chaque mise à jour, la DR ne correspond plus. On peut s'attendre à une invite « Kropa veut utiliser
  des informations confidentielles… » avec le mot de passe de session, pour chaque secret (déduit des sources, **à
  tester**). Avec le certificat auto-signé stable (§4.3), l'ACL reste valide.
- Repli prévu par le brief : le fichier chiffré par mot de passe maître si l'utilisateur refuse ces invites.

### 4.5 Canaux de distribution

| Canal | État | Conséquence |
|---|---|---|
| Cask Homebrew officiel | **impossible** | Homebrew 5.0 a déprécié les casks sans signature et annoncé la désactivation de ceux qui échouent au contrôle Gatekeeper ([brew 5.0][brew5]) ; confirmé en 6.0 ([brew 6.0][brew6]). Effectif : `copyq`, `makemkv`, `lyx` et `texstudio` sont `disabled`, `fails_gatekeeper_check`, depuis le 2026-09-01 (API formulae.brew.sh, consultée le 2026-11-11). `--no-quarantine` est supprimé. |
| Tap personnel, cask | possible, peu utile | Le `.dmg` téléchargé reste en quarantaine (« Ouvrir quand même »). Homebrew 6 demande d'**approuver explicitement** un tap tiers ([brew 6.0][brew6]). Des taps « unsigned » existent ([SoftwareRat][srtap]). |
| Tap personnel, formule compilée depuis les sources | possible | Pas de quarantaine, donc pas de Gatekeeper. Coût : Rust + outils en ligne de commande Xcode, compilation longue, et nouvelle DR ad hoc à chaque `brew upgrade` (permissions redemandées), sauf si on signe avec un certificat local (§4.3). La formule doit produire un `.app` (§4.2). Accueil d'une GUI dans homebrew/core : non vérifié, probablement refusé. |
| `.dmg` / `.zip` sur GitHub Releases | possible | Canal principal recommandé, signé avec le certificat auto-signé stable. Première ouverture et chaque mise à jour téléchargée : « Ouvrir quand même ». Un outil de mise à jour intégré qui télécharge sans poser la quarantaine éviterait cette étape (non vérifié). Il doit rester désactivé par défaut (principe « aucune requête réseau non configurée »). |
| `cargo install kropa` | possible pour les développeurs | Binaire nu : TCC mal listé sur 26.1 ([forum][f807898]), Info.plist absent (micro). À réserver à la CLI, sans promettre la GUI par ce canal. |

### 4.6 Ce que changerait un compte Apple Developer (99 $/an)

- **Gain net** : notarisation, donc ouverture par double-clic sans « Ouvrir quand même » ni mot de passe
  administrateur. Retour possible au **cask Homebrew officiel**. DR Developer ID stable (permissions et Keychain
  conservés, comme avec §4.3, mais sans clé maison à protéger). Keychain « data protection » accessible. Image de
  confiance. Option : Sparkle avec mises à jour fluides.
- **Ce qui ne change pas** : le rappel mensuel ER, le refus des raccourcis Option seule, les permissions à accorder
  une première fois, et `persistent-content-capture` (réservé au VNC, [M. Tsai][mjt]).
- Coût annexe : identité légale publiée dans le certificat, renouvellement annuel, notarisation dans la CI (quelques
  minutes par version, non vérifié).
- Avis de l'équipe : **sans compte**, l'outil reste utilisable par le public visé (développeurs, admins), au prix de
  2 ou 3 boîtes de dialogue par mise à jour si §4.3 tient. **Avec compte**, c'est le seul moyen d'atteindre un public
  non technique et le cask officiel. Décision à prendre par l'utilisateur.

## 5. Accès aux API Apple depuis Rust

| Option | Pour | Contre |
|---|---|---|
| **objc2 + crates générées** (`objc2` 0.6.5 du 2026-10-05 ; `objc2-app-kit`, `objc2-screen-capture-kit`, `objc2-vision`, `objc2-av-foundation`, `objc2-core-graphics`, `objc2-security` 0.3.2 du 2025-10-04, crates.io) | Rust pur, `cargo build` seul. Couverture du SDK 26 vérifiée : `SCScreenshotConfiguration`, `captureScreenshotWithFilter`, `setCaptureMicrophone`, `setShowMouseClicks` sont présents dans les sources de la 0.3.2. Gestion mémoire et blocs (`block2`) typés. Utilisé par Cap ([Cap][cap]) | Appels `unsafe` partout, à isoler dans un crate `kropa-macos`. Pas d'accès aux API réservées à Swift. Délai entre un SDK et sa génération. |
| Pont Swift (`swift-rs` 1.0.9, `swift-bridge` 0.1.59) | Accès à toutes les API, y compris celles réservées à Swift. async/await natif | Il faut Xcode et `swiftc` pour compiler, ce qui gêne la formule et `cargo install`. Deux langages et une FFI à maintenir seul. |
| `screencapturekit` 11.1.2 (crate de haut niveau) | API ergonomique, maintenue (2026-11-02) | Version majeure 11 : ruptures fréquentes. Ne couvre que SCK. |
| `cidre` 0.29.0 | Large couverture (SCK, AVF, VT), utilisé par Cap | Un seul mainteneur, version 0.x, licence non vérifiée. |

**Choix : objc2 et ses crates générées**, enfermés dans un crate de plateforme avec une API sûre. Un petit shim Swift
ne serait ajouté que si une API indispensable était réservée à Swift. Aucune ne l'est dans la matrice. Raccourcis :
`global-hotkey` 0.10.0. Presse-papiers : `arboard` 3.7.0. Keychain : `security-framework` 3.7.0 ou `keyring` 4.2.0
(vérifier qu'ils visent le Keychain à fichier). Versions relevées sur crates.io le 2026-11-11 ; licences et failles à
contrôler par `dependency-hygiene` avant adoption.

## 6. Points à transmettre aux équipes Architecture et Design

**Architecture**
- Livrer **un `.app`** même pour la CLI (TCC, Info.plist, micro). La CLI appelle le même binaire.
- Isoler la plateforme derrière un trait `Capture`/`Recorder`/`Overlay` : SCK sur macOS, portail XDG sur Linux.
- Signature : certificat auto-signé du projet en secret de CI, avec `codesign --options runtime` (runtime renforcé,
  utile seulement avec la notarisation, non vérifié pour l'ad hoc).
- Keychain à fichier, avec un repli par fichier chiffré (déjà au brief).
- Vidéo : `SCStream` vers `AVAssetWriter` (matériel, sans FFmpeg) sur macOS. FFmpeg seulement pour le GIF et la coupe
  si besoin, ce qui influe sur la licence choisie.
- Exclure de la capture les fenêtres de Kropa (miniature, épingles, overlay des touches) avec `SCContentFilter`.
- Pas d'outil de mise à jour actif par défaut. S'il existe, il est optionnel et documenté (quarantaine).

**Design**
- Écran d'accueil des permissions : une permission à la fois, au moment où elle sert, avec la raison. Prévoir
  l'écran « la permission semble active mais la capture échoue : retirez puis rajoutez Kropa ».
- Expliquer le rappel mensuel d'Apple, qui ne vient pas de Kropa.
- Le champ de raccourci refuse Option seule et Option+Maj (macOS 15+). Valeurs par défaut avec ⌘/⌃, sans conflit avec
  ⌘⇧3/4/5 du système.
- Page d'installation honnête : « Ouvrir quand même » en captures d'écran, alternative `xattr`, et conséquences.

## 7. Questions ouvertes

1. L'utilisateur prend-il un compte Developer (§4.6) ? Cela change le canal Homebrew et l'accueil des nouveaux
   utilisateurs.
2. Le certificat auto-signé conserve-t-il vraiment ER, Mic, SE, AX et les ACL Keychain sur 15, 26 et 27 ? Test sur
   machine réelle obligatoire avant d'en faire la base de la distribution.
3. « Ouvrir quand même » est-il redemandé à chaque version téléchargée, et pour une signature auto-signée ? À tester.
4. Le rappel mensuel ER existe-t-il toujours sur 26 et 27 (non vérifié) ?
5. Le refus des raccourcis Option seule a-t-il été levé en 15.2+ (sources contradictoires) ?
6. Faut-il garder Intel (universel) au-delà de Tahoe ? Pas de matériel de test Intel prévu (non vérifié).
7. Désaccord possible avec l'équipe Linux : sur macOS, la capture défilante sera manuelle en v1. Aligner l'expérience
   ou accepter l'écart ?
8. Absence de Mac de test en CI : quelle machine pour valider TCC, qui ne s'automatise pas ?

## Sources

[mr27]: https://www.macrumors.com/2026/09/10/macos-27-golden-gate-release-date/
[9to527]: https://9to5mac.com/2026/09/09/apple-confirms-macos-27-golden-gate-launch-date-september-14/
[sck-inrect]: https://developer.apple.com/documentation/screencapturekit/scscreenshotmanager/captureimage(in:completionhandler:)
[sck-shadow]: https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/ignoreshadowssinglewindow
[sck-rec]: https://developer.apple.com/documentation/screencapturekit/screcordingoutput
[sck-audio]: https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio
[sck-mic]: https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturemicrophone
[sck-clicks]: https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/showmouseclicks
[sck-picker]: https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker
[sck-cfg]: https://developer.apple.com/documentation/screencapturekit/scscreenshotconfiguration
[ca-tap]: https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:)
[nscs]: https://developer.apple.com/documentation/appkit/nscolorsampler
[vn-ocr]: https://developer.apple.com/documentation/vision/vnrecognizetextrequest
[vn-bc]: https://developer.apple.com/documentation/vision/vndetectbarcodesrequest
[fsaux]: https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/fullscreenauxiliary
[tn3127]: https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements
[tn3137]: https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
[f849969]: https://developer.apple.com/forums/thread/849969
[f839069]: https://developer.apple.com/forums/thread/839069
[f841530]: https://developer.apple.com/forums/thread/841530
[f819406]: https://developer.apple.com/forums/thread/819406
[f807898]: https://developer.apple.com/forums/thread/807898
[f763878]: https://developer.apple.com/forums/thread/763878
[shottr-seq]: https://shottr.cc/kb/sequoia
[es-opt]: https://blog.eternalstorms.at/2024/09/23/keyboard-shortcuts-using-option-and-or-shift-modifiers-only-no-longer-allowed-on-macos-sequoia/
[tidbits-m]: https://tidbits.com/2024/08/19/apple-reduces-excessive-sequoia-permission-requests-shifts-to-monthly
[mr151]: https://macrumors.com/2024/10/07/apple-screen-recording-popup-update
[mjt]: https://mjtsai.com/blog/2024/08/08/sequoia-screen-recording-prompts-and-the-persistent-content-capture-entitlement/
[dropshare]: https://support.dropshare.app/hc/en-us/articles/20871477694354--Dropshare-5-is-requesting-to-bypass-the-system-private-window-picker-and-directly-access-your-screen-and-audio
[moz-picker]: https://bugzilla.mozilla.org/show_bug.cgi?id=1918096
[ai-seq]: https://appleinsider.com/inside/macos-sequoia/tips/whats-changed-in-runtime-protection-for-macos-sequoia
[apple-open]: https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unidentified-developer-mh40616/mac
[apple-safe]: https://support.apple.com/en-au/102445
[el-nonot]: https://eclecticlight.co/2024/10/01/living-without-notarization/
[yabai]: https://github.com/koekeishiya/yabai/wiki/Installing-yabai-(from-HEAD)
[skhd]: https://raw.githubusercontent.com/jackielii/skhd.zig/main/docs/CODE_SIGNING.md
[nickboy]: https://nickboy-github-io.pages.dev/posts/tcc-cdhash-trap/
[brew5]: https://brew.sh/2025/11/12/homebrew-5.0.0/
[brew6]: https://brew.sh/2026/06/11/homebrew-6.0.0/
[srtap]: https://github.com/SoftwareRat/homebrew-unsigned-tap
[macshot]: https://github.com/sw33tLie/macshot
[cap]: https://github.com/CapSoftware/Cap

Autres outils consultés : Kap (Electron 13 + `aperture`, ancien, https://github.com/wulkano/Kap). CleanShot X et Shottr
sont propriétaires et signés : leur méthode interne n'est pas vérifiée. Shottr est touché par le refus des raccourcis
Option ([Shottr][shottr-seq]).
