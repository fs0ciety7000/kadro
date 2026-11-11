# Glossaire FR/EN de Kropa

Un terme, un sens, partout (CLI, interface, notifications, documentation). Base : `crates/kropa-i18n/i18n/*.ftl`,
`DESIGN.md` § 13 et `docs/brand/brand.md` § 2. Vouvoiement en français, pas de point d'exclamation, pas de majuscule
décorative. Orthographe anglaise : américain (color, canceled, authorize).

## Termes du produit

| FR | EN | Sens et usage | À éviter |
|---|---|---|---|
| capture | capture (nom) ; screenshot (l'image) | L'action et son résultat. EN : « screenshot » quand on parle de l'image, « capture » pour la commande et l'action. | cliché, snapshot, prise d'écran |
| écran | screen | Un moniteur. Sous Linux, le nom du connecteur ; commande `capture output`. | sortie, moniteur |
| zone | area | Partie de l'écran choisie par l'utilisateur. | région, sélection (pour la zone elle-même) |
| annoter, annotation | annotate, annotation | Dessin ou texte ajouté sur l'image (document `.kropa`). | |
| éditeur | editor | La fenêtre d'annotation. | |
| envoi, envoyer | upload (nom et verbe) | Transfert du fichier vers une destination. Le fichier reste local. | téléversement, push, publier |
| destination | destination | Où part le fichier (Zipline, S3, SFTP, HTTP). Terme affiché à l'utilisateur. | cible, serveur (sauf quand on parle du serveur lui-même) |
| profil | profile | Réglage enregistré d'une destination, désigné par son identifiant (`--to PROFIL`). Dans les messages, on dit « destination » ; « profil » sert aux options, identifiants et au magasin de secrets. | compte |
| destination par défaut | default destination | Celle que l'envoi utilise sans indication. | |
| lien | link | Adresse publique du fichier envoyé. Formats : url, markdown, html. | URL (sauf sens technique), permalien |
| URL, adresse | URL, address | URL pour les options et la réponse du serveur ; « adresse » dans les phrases courantes (adresse de la destination). | |
| presse-papiers | clipboard | Copie du lien ou de l'image. Jamais « pasteboard » en EN, même sur macOS. | tampon, pasteboard |
| copier | copy | « Lien copié » : dit seulement quand la copie est faite. | |
| historique | history | Captures et envois passés. | journal (réservé au journal technique) |
| file d'attente | queue | Envois en attente ou à relancer. | file d'envoi (ancienne formule) |
| tâche | job | Un élément de la file d'attente. | |
| relancer | retry | Refaire un envoi échoué. | |
| secret | secret | Jeton, mot de passe, clé : toute valeur qui ne doit pas fuiter. Jamais en argument, ni en variable d'environnement, ni réaffiché. | clé secrète, mot de passe (sauf phrase de passe du coffre) |
| magasin de secrets | secret store | Terme générique de l'endroit où sont les secrets (trousseau ou coffre). Son état : disponible, verrouillé, indisponible, ambigu, accès refusé, absent de cette version. | stockage, base |
| trousseau | keyring (Linux) ; Keychain (macOS) | Le trousseau du système : GNOME Keyring / KWallet via Secret Service, Trousseau d'accès (Keychain) sous macOS. Premier choix. | |
| coffre | vault | Le repli par fichier chiffré (`age`), protégé par une phrase de passe. Se déverrouille, se verrouille. | |
| courtier (du trousseau) | Keychain broker | Processus macOS qui sert le trousseau à l'application signée. Terme technique, peu affiché. | |
| phrase de passe | passphrase | Celle du coffre. | mot de passe |
| origine | origin | Schéma, hôte et port (`https://hôte[:port]`) auquel un secret peut être envoyé. | domaine |
| emplacement | place, placement (option) | Où le secret est placé dans la requête : `header:NOM`, `query:CLÉ`, `form:CHAMP`, `body`. EN : « place » dans les phrases, « placement » pour l'option `--placement`. | position |
| autoriser | authorize, allow | Étendre les origines ou emplacements d'un secret (jamais sans terminal). EN : « authorize » dans les aides, « allow » dans les invites. | |
| modèle de nom | file name template | Gabarit du nom de fichier (`{date}`, `{n}`…). | |
| variable | variable | Valeur de saisie d'un profil importé (`--var`) ou du modèle de nom. | |
| import, importer | import | Créer une destination depuis un fichier `.sxcu` ou TOML. | |
| diagnostic | diagnostic | Sortie de `kropa doctor`. | |
| capacité | capability | Ce que la plateforme permet : disponible, dégradée, indisponible. | |
| repli | fallback | Solution de remplacement documentée quand une capacité manque ; toujours dite, jamais silencieuse. | contournement |
| portail | portal | xdg-desktop-portal sous Linux. | |
| paquet `.app` | `.app` bundle | Application macOS. | « bundle » en français |
| lot | lot | Étape du plan (« arrive avec le lot 2.8 »). Terme de développement visible dans des messages provisoires ; à remplacer par une version avant la publication (voir points ouverts). | |

## Conventions de rédaction

- **Erreur** : ce qui s'est passé, puis quoi faire. Pas de blâme. Si rien n'a été envoyé ou copié, le dire (« Rien n'a
  été envoyé. ») ; si le fichier a pu partir, le dire aussi.
- **Succès** : affirmé seulement quand il est vérifié. Un état inconnu se dit inconnu (« la modification a pu être
  faite ou non »).
- **Valeurs saisies** : jamais réaffichées. Les messages citent des noms (option, profil, secret), jamais une valeur.
- **Commandes** : entre apostrophes droites, `'kropa dest list'`. **Noms de profil, de secret, de variable** : entre
  guillemets « » en français, apostrophes droites en anglais.
- **Français** : phrase courte, infinitif pour les aides d'option, participe passé pour les états (« Enregistré »).
  Espaces avant `: ; ? !` : espace ordinaire dans les chaînes de la CLI (convention existante, le terminal ne
  garantit pas l'insécable) ; espaces insécables (U+202F avant `; ? !`, U+00A0 avant `:`) dans l'interface graphique.
  Guillemets « ».
- **Anglais** : phrase courte, « respond/response » pour le serveur et le portail (pas « answer »), « Color »,
  « canceled ». Title Case réservé aux boutons et menus de l'interface graphique (DESIGN.md § 13), pas à la CLI.
- **Messages techniques** : un détail non traduit est marqué « Détail technique (non traduit) » / « Technical detail
  (not translated) ».
- **Invites** : `[o/N]` en français, `[y/N]` en anglais ; la réponse par défaut est toujours non.
