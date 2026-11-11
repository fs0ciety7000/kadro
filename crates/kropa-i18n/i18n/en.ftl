# English strings of the kropa command line.
# Naming: cmd-<path> command help, arg-<id> option help, val-<id> value name, err-* argument errors,
# error-* run-time errors, out-* normal output.

cli-about = Capture, annotate and upload screenshots.
cli-usage-heading = Usage:
cli-options-heading = Options:
cli-commands-heading = Commands:
cli-arguments-heading = Arguments:
cli-help-flag = Print help
cli-version-flag = Print version
cli-lang-flag = Language of messages (fr or en)

## Commands

cmd-capture = Take a screenshot
cmd-capture-screen = Capture all screens
cmd-capture-output = Capture one screen (the primary one by default)
cmd-capture-area = Capture a selected area
cmd-upload = Upload an existing file
cmd-dest = Manage upload destinations
cmd-dest-list = List destinations
cmd-dest-show = Show a destination (never its secrets)
cmd-dest-add = Add a destination
cmd-dest-edit = Edit a destination
cmd-dest-test = Send a test image to a destination
cmd-dest-remove = Remove a destination and its secrets
cmd-dest-set-default = Choose the default destination
cmd-dest-import = Import a destination from a .sxcu or TOML file
cmd-dest-export = Export destinations as TOML (secret references only)
cmd-secrets = Manage stored secrets
cmd-secrets-status = Show the secret store and missing secrets
cmd-secrets-set = Set the value of a secret
cmd-secrets-remove = Remove a secret
cmd-secrets-unlock = Unlock the vault for this session
cmd-secrets-lock = Lock the vault now
cmd-queue = Manage the upload queue
cmd-queue-list = List queued jobs
cmd-queue-retry = Retry a job, or all failed jobs
cmd-queue-cancel = Cancel a job
cmd-queue-clear = Remove finished jobs from the queue
cmd-history = Browse captures and uploads
cmd-history-list = List captures and uploads
cmd-edit = Open the editor on an image
cmd-doctor = Report the system, its capabilities and active fallbacks
cmd-version = Print version information

## Global options

arg-json = Machine-readable output: one JSON object on stdout
arg-quiet = Fewer messages on stderr (errors are kept)
arg-verbose = More messages on stderr (-vv: debug log)
arg-no-input = Never prompt, even on a terminal
arg-color = Colour of stderr output: auto, always or never
arg-config = Configuration folder
arg-password-fd = Read the vault passphrase on this file descriptor

## Options and arguments

arg-delay = Wait before capturing, 0 to 60 seconds (decimals accepted)
arg-output = Destination folder, or file if the path ends in an image extension
arg-name = File-name template (variables: date, time, host, mode, output, n)
arg-upload = Upload after saving, to the given profile or the default one
arg-no-upload = Do not upload, even if the configuration uploads by default
arg-copy = What goes to the clipboard: url, markdown, html, image or none
arg-upload-copy = What goes to the clipboard: url, markdown, html or none
arg-edit = Open the editor before the final save and upload
arg-var = Value of an input variable of an imported profile
arg-force = Overwrite an existing file
arg-last = Reuse the last area without the selection overlay
arg-connector = Screen name (connector name on Linux)
arg-file = File to process
arg-upload-file = File to upload
arg-edit-file = Image to edit (PNG or JPEG)
arg-dest-import-file = Profile file to import (.sxcu or .toml)
arg-to = Destination profile (default: the configured default)
arg-yes = Answer yes to confirmations (needed without a terminal)
arg-id = Profile identifier
arg-kind = Destination type: zipline, s3, sftp or http
arg-url = Base URL of the destination
arg-make-default = Make this the default destination
arg-bucket = S3 bucket
arg-region = S3 region
arg-endpoint = S3 endpoint URL
arg-path-style = S3 path-style addressing
arg-prefix = S3 key prefix
arg-public-base-url = Public base URL of the bucket
arg-presign = Lifetime of pre-signed URLs, 7 days at most (for example 12h, 7d)
arg-host = SFTP host
arg-port = SFTP port
arg-user = SFTP user
arg-remote-dir = SFTP remote folder
arg-public-url = Public base URL of the SFTP folder
arg-secret-stdin = Read the secret on stdin
arg-secret-fd = Read the secret on this file descriptor
arg-set = Set a profile key (repeatable); without options the profile opens in your editor
arg-none = Have no default destination
arg-as-id = Identifier for the imported profile
arg-ids = Profiles to export (all by default)
arg-profile = Profile identifier
arg-secret = Name of the secret
arg-job = Job identifier
arg-failed = Retry every failed job
arg-all = Include failed, done and cancelled jobs
arg-search = Text to look for in name, path, URL and profile
arg-limit = Maximum number of lines
arg-since = Only entries from this date (YYYY-MM-DD or RFC 3339)
arg-check = Ask GitHub Releases whether a newer version exists

val-delay = SEC
val-output = PATH
val-name = TEMPLATE
val-upload = PROFILE
val-to = PROFILE
val-copy = WHAT
val-var = NAME=VALUE
val-config = DIR
val-password-fd = N
val-secret-fd = N
val-connector = NAME
val-file = FILE
val-id = ID
val-kind = TYPE
val-set = KEY=VALUE
val-profile = PROFILE
val-secret = NAME
val-job = ID
val-ids = ID
val-search = TEXT
val-limit = N
val-since = DATE
val-presign = DURATION
val-lang = LANG

## Argument errors (the reason after "invalid value")

err-lang = unsupported language, available: fr, en
err-path = the path must not be empty
err-profile = a profile name uses 1 to 64 letters, digits, '-' or '_'
err-secret-name = a secret name uses 1 to 64 letters, digits, '-' or '_'
err-job = a job identifier uses 1 to 64 letters, digits, '-' or '_'
err-connector = a screen name must be 1 to 128 characters without control characters
err-delay = expected a number of seconds from 0 to 60
err-template = { $detail }
err-var = expected NAME=VALUE, where NAME starts with a letter or '_' and has only letters, digits and '_'
err-set = expected KEY=VALUE, where KEY has only letters, digits, '.', '-' and '_'
err-fd = expected a file descriptor number: 0, or 3 and above
err-port = expected a port number from 1 to 65535
err-limit = expected a whole number, 1 or more
err-since = expected a date (2026-11-11) or an RFC 3339 timestamp
err-url = expected an http:// or https:// URL with a host
err-presign = expected a duration of at most 7 days, such as 30m, 12h or 7d

## Usage errors

error-prefix = error:
error-hint = For more information, try '--help'.
error-tip = tip:
error-unknown-argument = unexpected argument '{ $arg }'
error-unknown-subcommand = unrecognized subcommand '{ $name }'
error-invalid-value = invalid value '{ $value }' for '{ $arg }'
error-possible-values = possible values: { $values }
error-value-validation = invalid value '{ $value }' for '{ $arg }': { $reason }
error-conflict = the argument '{ $arg }' cannot be used with '{ $other }'
error-missing-required = the following required arguments were not provided: { $args }
error-missing-subcommand = a subcommand is required
error-no-equals = a value for '{ $arg }' must be given as '{ $arg }=VALUE'
error-missing-value = a value is required for '{ $arg }'
error-name-with-file = --name cannot be used when --output is a file path

## Run-time errors

error-config = Invalid configuration: { $detail }
error-io = { $detail }
error-name = Invalid file name: { $detail }
error-secret-in-argument = Refused: the value of '{ $option }' looks like a secret. Secrets never go in arguments: use the hidden prompt, --secret-stdin or --secret-fd.
error-confirmation-required = A confirmation is required but input is not interactive. Pass --yes to confirm.
error-secret-input-required = A secret is required but input is not interactive. Use --secret-stdin or --secret-fd N.
error-no-default-destination = No destination was given and the configuration has no default_destination.
error-file-missing = File not found, or not a regular file: { $path }
error-output-not-dir = Not a folder: { $path }
error-file-exists = File already exists: { $path } (use --force to overwrite it)
error-unsupported-one = Not available yet: this command arrives with lot { $lots }.
error-unsupported-many = Not available yet: this command arrives with lots { $lots }.

## Normal output

out-version = kropa { $version }
out-version-commit = commit: { $commit }
out-version-target = target: { $target }
out-version-ipc = IPC protocol: not available yet (lot 2.1)
out-version-schemas = JSON schemas: { $schemas }
out-unknown = unknown
note-config = Configuration: { $path }
note-target = Target file: { $path }
note-target-deferred = Target file: chosen after the capture, in { $dir }
