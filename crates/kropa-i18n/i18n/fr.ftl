# Chaînes françaises de la ligne de commande kropa.
# Nommage : cmd-<chemin> aide d'une commande, arg-<id> aide d'une option, val-<id> nom de valeur,
# err-* erreurs d'argument, error-* erreurs d'exécution, out-* sortie normale.

cli-about = Capturer, annoter et envoyer des captures d'écran.
cli-usage-heading = Utilisation :
cli-options-heading = Options :
cli-commands-heading = Commandes :
cli-arguments-heading = Arguments :
cli-help-flag = Afficher l'aide
cli-version-flag = Afficher la version
cli-lang-flag = Langue des messages (fr ou en)

## Commandes

cmd-capture = Prendre une capture d'écran
cmd-capture-screen = Capturer tous les écrans
cmd-capture-output = Capturer un seul écran (l'écran principal par défaut)
cmd-capture-area = Capturer une zone sélectionnée
cmd-upload = Envoyer un fichier existant
cmd-dest = Gérer les destinations d'envoi
cmd-dest-list = Lister les destinations
cmd-dest-show = Afficher une destination (jamais ses secrets)
cmd-dest-add = Ajouter une destination
cmd-dest-edit = Modifier une destination
cmd-dest-test = Envoyer une image témoin vers une destination
cmd-dest-remove = Supprimer une destination et ses secrets
cmd-dest-set-default = Choisir la destination par défaut
cmd-dest-import = Importer une destination depuis un fichier .sxcu ou TOML
cmd-dest-export = Exporter les destinations en TOML (références de secrets seulement)
cmd-secrets = Gérer les secrets stockés
cmd-secrets-status = Afficher le magasin de secrets et le nombre d'entrées de chaque profil
cmd-secrets-set = Définir la valeur d'un secret
cmd-secrets-remove = Supprimer un secret
cmd-secrets-unlock = Déverrouiller le coffre pour cette session
cmd-secrets-lock = Verrouiller le coffre immédiatement
cmd-queue = Gérer la file d'envoi
cmd-queue-list = Lister les tâches en file
cmd-queue-retry = Relancer une tâche, ou toutes les tâches échouées
cmd-queue-cancel = Annuler une tâche
cmd-queue-clear = Retirer les tâches terminées de la file
cmd-history = Parcourir les captures et les envois
cmd-history-list = Lister les captures et les envois
cmd-edit = Ouvrir l'éditeur sur une image
cmd-doctor = Rapporter le système, ses capacités et les replis actifs
cmd-version = Afficher les informations de version

## Options globales

arg-json = Sortie lisible par une machine : un objet JSON sur stdout
arg-quiet = Moins de messages sur stderr (les erreurs restent)
arg-verbose = Plus de messages sur stderr (-vv : journal de débogage)
arg-no-input = Ne jamais poser de question, même sur un terminal
arg-color = Couleur de la sortie stderr : auto (par défaut), always ou never
arg-config = Dossier de configuration
arg-password-fd = Lire la phrase de passe du coffre sur ce descripteur de fichier

## Options et arguments

arg-delay = Attendre avant la capture, de 0 à 60 secondes (décimales acceptées)
arg-output = Dossier de destination, ou fichier si le chemin se termine par une extension d'image
arg-name = Modèle de nom de fichier (variables : date, time, host, mode, output, n)
arg-upload = Envoyer après l'enregistrement, vers le profil indiqué ou le profil par défaut
arg-no-upload = Ne pas envoyer, même si la configuration envoie par défaut
arg-copy = Ce qui va au presse-papiers : url, markdown, html, image ou none
arg-upload-copy = Ce qui va au presse-papiers : url, markdown, html ou none
arg-edit = Ouvrir l'éditeur avant l'enregistrement final et l'envoi
arg-var = Valeur d'une variable de saisie d'un profil importé
arg-force = Écraser un fichier existant
arg-last = Reprendre la dernière zone sans l'overlay de sélection
arg-connector = Nom de l'écran (nom du connecteur sous Linux)
arg-file = Fichier à traiter
arg-upload-file = Fichier à envoyer
arg-edit-file = Image à modifier (PNG ou JPEG)
arg-dest-import-file = Fichier de profil à importer (.sxcu ou .toml)
arg-to = Profil de destination (par défaut : le profil par défaut configuré)
arg-yes = Répondre oui aux confirmations (nécessaire sans terminal)
arg-id = Identifiant du profil
arg-kind = Type de destination : zipline, s3, sftp ou http
arg-url = URL de base de la destination
arg-make-default = En faire la destination par défaut
arg-bucket = Bucket S3
arg-region = Région S3
arg-endpoint = URL du point d'accès S3
arg-path-style = Adressage S3 de type chemin
arg-prefix = Préfixe des clés S3
arg-public-base-url = URL publique de base du bucket
arg-presign = Durée de vie des URL présignées, 7 jours au plus (par exemple 12h, 7d)
arg-host = Hôte SFTP
arg-port = Port SFTP
arg-user = Utilisateur SFTP
arg-remote-dir = Dossier distant SFTP
arg-public-url = URL publique de base du dossier SFTP
arg-secret-stdin = Lire le secret sur stdin
arg-secret-fd = Lire le secret sur ce descripteur de fichier
arg-set = Définir une clé du profil (répétable) ; sans option, le profil s'ouvre dans votre éditeur
arg-none = Ne pas avoir de destination par défaut
arg-as-id = Identifiant du profil importé
arg-ids = Profils à exporter (tous par défaut)
arg-profile = Identifiant du profil
arg-secret = Nom du secret
arg-job = Identifiant de la tâche
arg-failed = Relancer toutes les tâches échouées
arg-all = Inclure les tâches échouées, terminées et annulées
arg-search = Texte à chercher dans le nom, le chemin, l'URL et le profil
arg-limit = Nombre maximal de lignes
arg-since = Seulement les entrées depuis cette date (AAAA-MM-JJ ou RFC 3339)
arg-check = Demander à GitHub Releases s'il existe une version plus récente

val-delay = SEC
val-output = CHEMIN
val-name = MODÈLE
val-upload = PROFIL
val-to = PROFIL
val-copy = CIBLE
val-var = NOM=VALEUR
val-config = DOSSIER
val-password-fd = N
val-secret-fd = N
val-connector = NOM
val-file = FICHIER
val-id = ID
val-kind = TYPE
val-set = CLÉ=VALEUR
val-profile = PROFIL
val-secret = NOM
val-job = ID
val-ids = ID
val-search = TEXTE
val-limit = N
val-since = DATE
val-presign = DURÉE
val-lang = LANGUE
val-command = COMMANDE
val-color = COULEUR

## Erreurs d'argument (la raison après « valeur invalide »)

err-lang = langue non prise en charge, disponibles : fr, en
err-path = le chemin ne doit pas être vide
err-profile = un nom de profil comporte de 1 à 64 lettres, chiffres, '-' ou '_'
err-secret-name = un nom de secret comporte de 1 à 64 lettres, chiffres, '-' ou '_'
err-job = un identifiant de tâche comporte de 1 à 64 lettres, chiffres, '-' ou '_'
err-connector = un nom d'écran comporte de 1 à 128 caractères, sans caractère de contrôle
err-delay = un nombre de secondes de 0 à 60 est attendu
err-template = { $detail }
err-template-variable = variable inconnue (disponibles : date, time, n, w, h, kind, mode, output, host)
err-template-argument = argument invalide pour la variable { $detail }
err-var = NOM=VALEUR attendu, où NOM commence par une lettre ou '_' et ne contient que des lettres, chiffres et '_'
err-set = CLÉ=VALEUR attendu, où CLÉ ne contient que des lettres, chiffres, '.', '-' et '_'
err-fd = un numéro de descripteur de fichier est attendu : 0, ou 3 et plus
err-port = un numéro de port de 1 à 65535 est attendu
err-limit = un nombre entier d'au moins 1 est attendu
err-since = une date (2026-11-11) ou un horodatage RFC 3339 est attendu
err-url = une URL http:// ou https:// avec un hôte est attendue
err-presign = une durée de 7 jours au plus est attendue, par exemple 30m, 12h ou 7d

## Erreurs d'usage

error-prefix = erreur :
error-hint = Pour plus d'informations, essayez '--help'.
error-tip = astuce :
error-unknown-argument = argument inattendu '{ $arg }'
error-extra-argument = argument en trop
error-unknown-subcommand = sous-commande inconnue
error-invalid-value = valeur invalide pour '{ $arg }'
error-possible-values = valeurs possibles : { $values }
error-value-validation = valeur invalide pour '{ $arg }' : { $reason }
error-conflict = l'argument '{ $arg }' ne peut pas être utilisé avec '{ $other }'
error-missing-required = les arguments obligatoires suivants n'ont pas été fournis : { $args }
error-missing-subcommand = une sous-commande est obligatoire
error-no-equals = la valeur de '{ $arg }' doit être donnée sous la forme '{ $arg }=VALEUR'
error-missing-value = une valeur est obligatoire pour '{ $arg }'
error-too-many-values = trop de valeurs pour '{ $arg }'
error-usage-generic = ligne de commande invalide
error-name-with-file = --name est inutilisable quand --output est un chemin de fichier

## Erreurs d'exécution

error-config = Configuration invalide : { $detail }
error-io = { $detail }
error-name = Nom de fichier invalide : { $detail }
error-secret-in-argument = Refusé : la valeur de '{ $option }' ressemble à un secret. Un secret ne passe jamais en argument : utilisez la saisie masquée, --secret-stdin ou --secret-fd.
error-confirmation-required = Une confirmation est nécessaire mais l'entrée n'est pas interactive. Ajoutez --yes pour confirmer.
error-secret-input-required = Un secret est nécessaire mais l'entrée n'est pas interactive. Utilisez --secret-stdin ou --secret-fd N.
error-no-default-destination = Aucune destination indiquée et la configuration n'a pas de default_destination.
error-file-missing = Fichier introuvable, ou ce n'est pas un fichier ordinaire : { $path }
error-output-not-dir = Ce n'est pas un dossier : { $path }
error-file-exists = Le fichier existe déjà : { $path } (--force pour l'écraser)
error-unsupported-one = Pas encore disponible : cette commande arrive avec le lot { $lots }.
error-unsupported-many = Pas encore disponible : cette commande arrive avec les lots { $lots }.

## Sortie normale

out-version = kropa { $version }
out-version-commit = commit : { $commit }
out-version-target = cible : { $target }
out-version-ipc = protocole IPC : pas encore disponible (lot 2.1)
out-version-schemas = schémas JSON : { $schemas }
out-unknown = inconnu
note-config = Configuration : { $path }
note-target = Fichier cible : { $path }
note-target-deferred = Fichier cible : choisi après la capture, dans { $dir }
note-saved = Enregistré : { $path } ({ $width }x{ $height })
note-delay = Attente de { $seconds } s avant la capture
note-app-id-registered = Enregistré auprès du portail sous l'identifiant { $app_id }
note-app-id-registration-unavailable = Le portail n'a pas de registre d'applications : les permissions peuvent ne pas être liées à { $app_id }

## Erreurs de capture (error-capture-<raison>)

error-capture-no-session-bus = Pas de session graphique à capturer : le bus de session D-Bus est injoignable. Lancez kropa depuis un terminal de votre session de bureau.
error-capture-portal-unavailable = Le portail de capture d'écran n'est pas disponible. Installez xdg-desktop-portal et le module de portail de votre bureau (xdg-desktop-portal-gnome, -kde ou -wlr), puis rouvrez votre session.
error-capture-portal-declined = Le portail de capture d'écran a refusé la demande. Si vous avez refusé la permission, autorisez-la de nouveau dans les réglages de confidentialité de votre bureau ou réinitialisez-la (flatpak permission-reset) ; sinon, vérifiez que le module de portail de votre bureau fonctionne.
error-capture-portal-dismissed = La demande de capture a été fermée avant la prise de vue. Relancez la commande et autorisez la capture.
error-capture-portal-not-allowed = Le portail n'autorise pas Kropa à faire des captures d'écran. Autorisez-le dans les réglages de confidentialité de votre bureau ou réinitialisez la permission, puis réessayez.
error-capture-output-not-isolable = Le portail de capture d'écran rend tous les écrans en une seule image et ne peut pas en isoler un. Utilisez plutôt 'kropa capture screen'.
error-capture-image-format-unsupported = Les captures ne sont fournies qu'en images PNG : le format { $detail } n'est pas encore pris en charge. Utilisez png dans la configuration, ou un nom de fichier en .png.
error-capture-portal-error = Le portail de capture d'écran a échoué : { $detail }
error-capture-portal-no-response = Le portail de capture d'écran n'a pas répondu.
error-capture-portal-invalid-response = Le portail de capture d'écran a répondu avec un emplacement inutilisable : { $detail }
error-capture-portal-file-unreadable = La capture donnée par le portail est illisible : { $detail }
error-capture-invalid-image = Le portail a rendu une image invalide : { $detail }
error-capture-screen-recording-denied = Kropa n'a pas le droit d'enregistrer l'écran. Ouvrez Réglages Système > Confidentialité et sécurité > Enregistrement de l'écran et de l'audio système, activez Kropa (ajoutez-le avec « + » s'il n'est pas dans la liste), puis relancez la commande. macOS applique le changement aux programmes lancés ensuite. Lancé depuis Terminal, macOS peut lister Terminal à la place de Kropa : autorisez celui-là.
error-capture-screen-recording-stale = macOS affiche la permission d'enregistrement de l'écran comme accordée mais refuse la capture. Cela arrive quand Kropa a été remplacé par une version signée autrement. Ouvrez Réglages Système > Confidentialité et sécurité > Enregistrement de l'écran et de l'audio système, retirez Kropa avec « - », ajoutez-le de nouveau avec « + », puis relancez la commande.
error-capture-macos-too-old = Kropa demande macOS 15 (Sequoia) ou plus récent.
error-capture-no-graphical-session = Pas de session graphique à capturer : ce programme ne tourne pas dans une session macOS ouverte (ssh, service en arrière-plan). Lancez kropa depuis un terminal sur le Mac lui-même, session ouverte.
error-capture-output-not-found = Écran introuvable : { $detail }. Lancez 'kropa doctor' pour lister les écrans, puis donnez un nom ou un identifiant de cette liste.
error-capture-screencapturekit-error = ScreenCaptureKit a échoué : { $detail }
error-capture-screencapturekit-no-response = ScreenCaptureKit n'a pas répondu à temps. Une boîte de dialogue de macOS attend peut-être votre réponse : si oui, répondez-y puis relancez la commande.
error-capture-image-encoding-failed = L'image n'a pas pu être convertie en fichier PNG : { $detail }
error-write = Écriture impossible de { $path } : { $detail }

## kropa doctor

doctor-title = Diagnostic de Kropa
doctor-section-system = Système
doctor-system = { $platform } ({ $arch }), kropa { $version }
doctor-section-capabilities = Capacités
doctor-capability = { $feature } : { $status }
doctor-why = Pourquoi : { $reason }
doctor-instead = À la place : { $fallback }
doctor-feature-capture-screen = capture d'écran (screen)
doctor-feature-capture-output = capture d'un écran (output)
doctor-status-available = disponible
doctor-status-degraded = dégradée
doctor-status-unavailable = indisponible
doctor-reason-no-session-bus = pas de bus de session D-Bus : lancez kropa dans une session graphique
doctor-reason-portal-unavailable = le portail de capture d'écran manque : installez xdg-desktop-portal et le module de votre bureau
doctor-reason-output-not-isolable = le portail rend tous les écrans en une seule image
doctor-reason-no-graphical-session = pas de session macOS ouverte : lancez kropa depuis un terminal sur le Mac lui-même
doctor-reason-screen-recording-denied = la permission d'enregistrement de l'écran manque : Réglages Système > Confidentialité et sécurité > Enregistrement de l'écran et de l'audio système
doctor-reason-macos-too-old = macOS 15 ou plus récent est requis
doctor-fallback-capture-screen-instead = utilisez 'kropa capture screen' (tous les écrans en une image)
doctor-section-details = Détails
doctor-fact = { $label } : { $value }
doctor-value-yes = oui
doctor-value-no = non
doctor-value-absent = introuvable
doctor-value-not-probed = pas encore vérifié
doctor-value-unreachable = non vérifié : pas de bus de session
doctor-fact-desktop = Bureau (à titre indicatif, jamais utilisé pour choisir un moteur)
doctor-fact-session-type = Type de session
doctor-fact-wayland-display = WAYLAND_DISPLAY défini
doctor-fact-x11-display = DISPLAY défini
doctor-fact-session-bus = Bus de session D-Bus joignable
doctor-fact-session-bus-error = Erreur du bus de session
doctor-fact-portal-screenshot = Portail Screenshot, version de l'interface
doctor-fact-portal-screenshot-targets = Cibles du portail Screenshot
doctor-fact-portal-screencast = Portail ScreenCast, version de l'interface
doctor-fact-portal-global-shortcuts = Portail GlobalShortcuts, version de l'interface
doctor-fact-portal-secret = Portail Secret, version de l'interface
doctor-fact-portal-registry = Registre des applications hôtes, version de l'interface
doctor-fact-kwin-service = KWin sur le bus de session (son interface ScreenShot2 n'est pas utilisée)
doctor-fact-app-id = Identifiant d'application
doctor-fact-desktop-file = Fichier .desktop de l'identifiant installé (scripts/dev-install.sh)
doctor-fact-layer-shell = Wayland layer-shell
doctor-fact-data-control = Wayland data-control (presse-papiers)
doctor-fact-macos-version = Version de macOS
doctor-fact-graphical-session = Session ouverte avec serveur de fenêtres
doctor-fact-screen-recording = Permission d'enregistrement de l'écran accordée (lue sans la demander)
doctor-fact-app-bundle = Lancé depuis un bundle .app
doctor-fact-bundle-identifier = Identifiant du bundle
doctor-fact-display-primary = Écran (principal, utilisé par 'capture output' sans nom)
doctor-fact-display = Écran
doctor-section-paths = Chemins
doctor-path-config = Fichier de configuration
doctor-path-config-error = Erreur de configuration
doctor-path-captures = Dossier des captures
doctor-section-pending = Pas encore disponible
doctor-pending-app = Socket et version de kropa-app : lot 2.1
doctor-pending-clipboard = Presse-papiers : lot 1.10
doctor-pending-outputs = Écrans et leurs noms : lot 2.5

## Envoi et destinations (lot 1.7)

arg-expect-host = Hôte que le fichier doit viser ; obligatoire avec --yes sans terminal
val-expect-host = HÔTE
err-host = nom d'hôte attendu, sans schéma ni chemin, par exemple z.example.org

error-cancelled = Annulé.
error-import-host-required = Importer sans terminal exige --yes et --expect-host HÔTE, pour qu'un fichier ne puisse pas vous envoyer vers un hôte que vous n'avez pas choisi.
error-import-file-type = Type de fichier inconnu : un fichier .sxcu ou .toml est attendu.
error-dest-exists = Une destination nommée '{ $id }' existe déjà, ou le magasin de secrets garde encore des entrées sous ce nom. Choisissez un autre identifiant avec --id.
error-dest-no-file = Impossible de lire le fichier : { $detail }

error-upload-invalid-destination = Destination invalide : { $detail }
error-upload-unknown-destination = { $detail }. Listez-les avec 'kropa dest list'.
error-upload-import-invalid = Import impossible : { $detail }
error-upload-import-unsupported = Import impossible : { $detail }
error-upload-import-too-large = Import impossible : le fichier dépasse 64 Kio.
error-upload-missing-variable = La destination attend une valeur pour '{ $detail }' : ajoutez --var { $detail }=VALEUR.
error-upload-invalid-variable = La valeur donnée pour '{ $detail }' ne fait pas partie des choix de la destination.
error-upload-secret-missing = Le secret '{ $detail }' de la destination n'est pas renseigné.
error-upload-secret-store-not-delivered = Cette version n'a pas encore de stockage des secrets : sous macOS il arrive avec le lot 1.8m (le courtier Keychain). Rien n'a été fait.
error-upload-secret-store-error = Le magasin de secrets a échoué : { $detail }
error-upload-timeout = Le serveur n'a pas répondu à temps. Le fichier est conservé.
error-upload-connect = Connexion au serveur impossible. Vérifiez l'adresse et le réseau. Le fichier est conservé.
error-upload-tls = La connexion sécurisée a échoué : le certificat du serveur n'a pas pu être vérifié. S'il vient d'une autorité privée, ajoutez-la au magasin de certificats du système. Le fichier est conservé.
error-upload-network = La requête a échoué. Le fichier est conservé.
error-upload-http-status = Le serveur a répondu avec le statut { $status } : { $detail }
error-upload-http-status-bare = Le serveur a répondu avec le statut { $status }.
error-upload-url-missing = Le serveur a répondu, mais sa réponse ne contient pas d'URL. Vérifiez le gabarit de réponse de la destination.
error-upload-url-invalid = Le serveur a répondu avec une URL inutilisable (ni http ni https, ou avec des caractères de contrôle). Rien n'a été copié.
error-upload-response-too-large = La réponse du serveur dépasse 1 Mio : elle n'a pas été lue.
error-upload-bad-response = La réponse du serveur est illisible : { $detail }
error-upload-file-too-large = Le fichier dépasse la limite de la destination (max_size).
error-upload-origin-not-allowed = Refusé : { $detail }. Rien n'a été envoyé.
error-upload-placement-not-allowed = Refusé : { $detail }. Rien n'a été envoyé.
error-upload-secret-in-output = Refusé : la réponse du serveur contient le secret, son lien est donc retenu et rien n'a été copié. Le fichier a peut-être été envoyé.
error-upload-https-required = Refusé : { $detail }. Rien n'a été envoyé.
error-upload-cross-origin-redirect = Refusé : le serveur a redirigé vers une autre origine ; la requête n'y a pas été répétée.
error-upload-secret-in-argument = Refusé : { $detail }
error-upload-secret-too-short = Refusé : un secret fait au moins 4 octets.
error-upload-secret-in-path = Refusé : { $detail }
error-upload-import-host-mismatch = Refusé : le fichier vise { $detail }, pas l'hôte attendu. Rien n'a été importé.
error-upload-io = { $detail }

warn-secret-short = Le secret '{ $detail }' est court : il peut être masqué à plus d'endroits que nécessaire.
warn-secret-echoed = Le serveur a répété un secret dans sa réponse : le texte a été masqué.
warn-uploaded-link-withheld = Le fichier a peut-être été envoyé, mais son lien est retenu parce qu'il contenait un secret.
warn-thumbnail-url-invalid = L'URL de miniature de la réponse est inutilisable et a été écartée.
warn-deletion-url-invalid = L'URL de suppression de la réponse est inutilisable et a été écartée.
warn-metadata-not-stripped = Les métadonnées ne sont pas encore retirées du fichier envoyé (lot 4.8).
warn-clipboard-unavailable = Lien non copié : le presse-papiers n'est pas encore disponible (lot 1.10). Le lien est affiché ci-dessus.

note-uploaded = Envoyé vers { $profile }
note-link = Lien ({ $format }) : { $text }
note-no-destinations = Aucune destination configurée. Importez-en une avec 'kropa dest import FICHIER'.

out-dest-default-marker = (par défaut)
out-show-title = { $id } ({ $type })
out-show-label = Libellé : { $label }
out-show-address = Adresse : { $address }
out-show-origin = Origine : { $origin }
out-show-kinds = Accepte : { $kinds }
out-show-default = Destination par défaut : oui
out-show-request = Requête : { $method } { $url }
out-show-header = En-tête { $name } : { $value }
out-show-query = Paramètre { $name } : { $value }
out-show-field = Champ { $name } : { $value }
out-show-secret = Secret { $name } : { $state }
out-show-var = Variable { $name }
out-secret-set = renseigné
out-secret-missing = manquant
out-secret-unknown = inconnu (pas de stockage des secrets dans cette version : lot 1.8m)

prompt-dest-test = Envoyer une image de test de 1x1 vers { $id } ({ $origin }) ? [o/N]
prompt-dest-import = Créer la destination '{ $id }' pour { $origin } ? [o/N]
out-test-status = Statut : { $status }
out-test-header = { $name } : { $value }
out-test-url = URL : { $url }
out-test-link = Lien ({ $format }) : { $text }

out-import-summary = Destination '{ $id }' pour { $origin }
out-import-http-warning = Cette adresse n'est pas en https.
out-import-secret = Secret { $name } ({ $placement }) : { $preview }
out-import-note-text-types-ignored = Ignoré : le fichier déclare aussi { $detail }, que Kropa n'envoie pas.
out-import-note-legacy-syntax = Le fichier utilise l'ancienne syntaxe de ShareX ; elle a été migrée.
out-import-note-query-secret = Un secret ({ $detail }) passe dans les paramètres de l'adresse : il apparaîtra dans les journaux du serveur et des proxys.
out-import-note-body-template-secret = Le corps de la requête a été gardé comme secret ({ $detail }) et part tel quel : ses variables ne sont pas remplacées.
note-imported = Destination '{ $id }' importée ({ $origin }).

## Secrets (lot 1.8)

cmd-secrets-allow = Autoriser un secret pour une autre origine ou un autre emplacement (demande sur un terminal)
arg-origin = Origine à laquelle le secret peut aussi être envoyé (https://hôte[:port])
val-origin = URL
arg-placement = Emplacement où le secret peut aussi être placé : header:NOM, query:CLÉ, form:CHAMP ou body
val-placement = EMPLACEMENT
arg-secrets-allow-yes = Sans effet : cette confirmation n'est jamais donnée sans terminal
err-placement = un emplacement comme header:NOM, query:CLÉ, form:CHAMP ou body est attendu
error-secret-undeclared = La destination « { $profile } » ne déclare pas de secret nommé « { $name } ».
error-secret-no-origin = L'adresse de la destination n'est pas une origine valide (http ou https, sans user:pass@).
error-secret-too-long = Le secret dépasse 4 Kio.
error-secret-not-text = Le secret n'est pas du texte UTF-8 valide.
error-secret-unreadable = Impossible de lire le secret sur stdin ou sur le descripteur donné.
error-secrets-no-body = La destination n'a pas de corps de requête où placer un secret.
error-secrets-bad-placement = Refusé : un secret ne se place que dans header:NOM, query:CLÉ, form:CHAMP ou body, jamais dans l'hôte ou le chemin de l'adresse.
error-secrets-bad-origin = Origine invalide : une adresse http:// ou https:// sans user:pass@ est attendue.
error-allow-interactive = Autoriser un secret pour une nouvelle origine ou un nouvel emplacement demande un terminal et n'est jamais confirmé par --yes.
error-upload-keyring-locked = Le trousseau du système est verrouillé et ne peut pas être déverrouillé d'ici ({ $detail }). Déverrouillez-le (ouvrez votre session de bureau ou votre gestionnaire de trousseau) puis réessayez.
error-upload-keyring-timeout = Le trousseau du système n'a pas répondu à temps ({ $detail }).
error-upload-keyring-ambiguous = Le trousseau du système contient plusieurs fois le même secret ({ $detail }). Supprimez les doublons avec votre gestionnaire de trousseau (cherchez « io.github.fs0ciety7000.Kropa »).
error-upload-keyring-unavailable = Aucun trousseau système utilisable ({ $detail }). Le coffre de repli « age » (kropa secrets use vault) arrive avec le lot 2.8 ; rien ne bascule tout seul.
out-secrets-none = aucun
out-secrets-store = Magasin de secrets : { $store } ({ $state })
out-secrets-state-available = disponible
out-secrets-state-locked = verrouillé
out-secrets-state-unavailable = indisponible
out-secrets-state-ambiguous = ambigu
out-secrets-state-denied = accès refusé
out-secrets-state-not_delivered = absent de cette version
out-secrets-detail = { $detail }
out-secrets-profile = { $profile } : { $entries } entrées, dont { $empty } encore vides
out-secrets-fallback = Le coffre de repli « age » (kropa secrets use vault) arrive avec le lot 2.8 ; rien ne bascule tout seul.
out-secrets-new-entry = Nouveau secret { $profile }/{ $name } : les endroits où il peut aller sont fixés maintenant.
out-secrets-current = Secret { $profile }/{ $name } aujourd'hui :
out-secrets-origins = Envoyé seulement à : { $origins }
out-secrets-placements = Placé seulement dans : { $placements }
out-secrets-query-warning = Un secret dans la query d'une adresse finit dans les journaux du serveur et des proxys.
out-secrets-add-origin = Ajout de l'origine : { $origin }
out-secrets-add-placement = Ajout de l'emplacement : { $placement }
prompt-secret-value = Valeur de { $profile }/{ $name } (masquée) :
prompt-secrets-set = Ranger le secret { $profile }/{ $name } avec ces limites ? [o/N]
prompt-secrets-remove = Supprimer le secret { $profile }/{ $name } ? [o/N]
prompt-secrets-allow = Autoriser cela pour le secret { $profile }/{ $name } ? [o/N]
note-secret-set = Secret { $profile }/{ $name } rangé.
note-secret-removed = Secret { $profile }/{ $name } supprimé.
note-secret-absent = Il n'y avait pas de secret { $profile }/{ $name }.
note-secret-allowed = Secret { $profile }/{ $name } mis à jour.
