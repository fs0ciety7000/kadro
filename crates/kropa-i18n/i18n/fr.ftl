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
cmd-secrets-status = Afficher le magasin de secrets et les secrets manquants
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
error-unknown-subcommand = sous-commande inconnue '{ $name }'
error-invalid-value = valeur invalide '{ $value }' pour '{ $arg }'
error-possible-values = valeurs possibles : { $values }
error-value-validation = valeur invalide '{ $value }' pour '{ $arg }' : { $reason }
error-conflict = l'argument '{ $arg }' ne peut pas être utilisé avec '{ $other }'
error-missing-required = les arguments obligatoires suivants n'ont pas été fournis : { $args }
error-missing-subcommand = une sous-commande est obligatoire
error-no-equals = la valeur de '{ $arg }' doit être donnée sous la forme '{ $arg }=VALEUR'
error-missing-value = une valeur est obligatoire pour '{ $arg }'
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
