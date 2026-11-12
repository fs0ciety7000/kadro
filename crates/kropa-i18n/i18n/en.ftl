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
cmd-secrets-status = Show the secret store and how many entries each profile has
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
arg-color = Color of stderr output: auto (default), always or never
arg-config = Configuration folder
arg-password-fd = Read the vault passphrase on this file descriptor

## Options and arguments

arg-delay = Wait before capturing, 0 to 60 seconds (decimals accepted)
arg-output = Destination folder, or file if the path ends in an image extension
arg-name = File name template (variables: date, time, n, w, h, kind, mode, output, host)
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
arg-kind = Destination type: zipline, s3 or sftp (an HTTP API is described in a file: 'kropa dest import')
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
arg-none = Clear the default destination
arg-as-id = Identifier for the imported profile
arg-ids = Profiles to export (all by default)
arg-profile = Profile identifier
arg-secret = Name of the secret
arg-job = Job identifier
arg-failed = Retry every failed job
arg-all = Include failed, done and canceled jobs
arg-search = Text to look for in name, path, URL and profile
arg-limit = Maximum number of lines
arg-since = Only entries from this date (YYYY-MM-DD or RFC 3339)
arg-check = Ask GitHub Releases whether a newer version exists
arg-reset-screenshot-permission = Make the system forget the stored screenshot permission (the next capture asks again), instead of reporting

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
val-command = COMMAND
val-color = COLOR

## Argument errors (the reason after "invalid value")

err-lang = unsupported language, available: fr, en
err-path = the path must not be empty
err-profile = a profile name uses 1 to 64 letters, digits, '-' or '_'
err-secret-name = a secret name uses 1 to 64 letters, digits, '-' or '_'
err-job = a job identifier uses 1 to 64 letters, digits, '-' or '_'
err-connector = a screen name must be 1 to 128 characters without control characters
err-delay = expected a number of seconds from 0 to 60
err-template = { $detail }
err-template-variable = unknown variable (available: date, time, n, w, h, kind, mode, output, host)
err-template-argument = invalid argument for the variable { $detail }
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
error-extra-argument = unexpected extra argument
error-unknown-subcommand = unrecognized subcommand
error-invalid-value = invalid value for '{ $arg }'
error-possible-values = possible values: { $values }
error-value-validation = invalid value for '{ $arg }': { $reason }
error-conflict = the argument '{ $arg }' cannot be used with '{ $other }'
error-missing-required = the following required arguments were not provided: { $args }
error-missing-subcommand = a subcommand is required
error-no-equals = a value for '{ $arg }' must be given as '{ $arg }=VALUE'
error-missing-value = a value is required for '{ $arg }'
error-too-many-values = too many values for '{ $arg }'
error-usage-generic = invalid command line
error-name-with-file = --name cannot be used when --output is a file path

## Run-time errors

error-config = Invalid configuration: { $detail }
error-io = { $detail }
error-name = Invalid file name: { $detail }
error-secret-in-argument = Refused: the value of '{ $option }' looks like a secret. Secrets never go in arguments: use the hidden prompt, --secret-stdin or --secret-fd.
error-confirmation-required = A confirmation is required but input is not interactive. Pass --yes to confirm.
error-secret-input-required = A secret is required but input is not interactive. Use --secret-stdin or --secret-fd N.
error-no-default-destination = No destination was given, and the configuration has no default_destination. Give a profile, or choose a default destination with 'kropa dest set-default'.
error-file-missing = File not found, or not a regular file: { $path }
error-output-not-dir = Not a folder: { $path }
error-file-exists = File already exists: { $path } (use --force to overwrite it)
error-unsupported-one = Not available yet: this command arrives with lot { $lots }.
error-unsupported-many = Not available yet: this command arrives with lots { $lots }.

## Normal output

out-version = kropa { $version }
out-version-commit = commit: { $commit }
out-version-target = target: { $target }
out-version-ipc = IPC protocol: { $protocol }
out-version-schemas = JSON schemas: { $schemas }
out-unknown = unknown
note-config = Configuration: { $path }
note-target = Target file: { $path }
note-target-deferred = Target file: chosen after the capture, in { $dir }
note-saved = Saved: { $path } ({ $width }x{ $height })
note-delay = Waiting { $seconds } s before the capture
note-app-id-registered = Registered with the portal as { $app_id }
note-app-id-registration-unavailable = The portal has no application registry: permissions may not be tied to { $app_id }

## Capture errors (error-capture-<reason>)

error-capture-no-session-bus = No graphical session to capture: the D-Bus session bus is not reachable. Run kropa from a terminal inside your desktop session.
error-capture-portal-unavailable = The screenshot portal is not available. Install xdg-desktop-portal and the portal backend of your desktop (xdg-desktop-portal-gnome, -kde or -wlr), then log in again.
error-capture-portal-declined = The screenshot portal declined the request. If you refused the permission, allow it again in the privacy settings of your desktop, or make the system forget the refusal with 'kropa doctor --reset-screenshot-permission' and run the command again; otherwise check that the portal backend of your desktop works.
error-capture-portal-dismissed = The screenshot request was dismissed before the screenshot was taken. Run the command again and allow the screenshot.
error-capture-portal-not-allowed = The portal does not allow Kropa to take screenshots. Allow it in the privacy settings of your desktop, or make the system forget the refusal with 'kropa doctor --reset-screenshot-permission', then try again.
error-capture-output-not-isolable = The screenshot portal returns all screens as one image and cannot isolate one screen. Use 'kropa capture screen' instead.
error-capture-image-format-unsupported = Screenshots are delivered as PNG images only: the format { $detail } is not supported yet. Use png in the configuration, or a .png file name.
error-capture-portal-error = The screenshot portal failed: { $detail }
error-capture-portal-no-response = The screenshot portal did not respond.
error-capture-portal-invalid-response = The screenshot portal returned a location that cannot be used: { $detail }
error-capture-portal-file-unreadable = The screenshot given by the portal cannot be read: { $detail }
error-capture-invalid-image = The portal returned an invalid image: { $detail }
error-capture-screen-recording-denied = Kropa is not allowed to record the screen. Open System Settings > Privacy & Security > Screen & System Audio Recording, switch Kropa on (add it with "+" if it is not listed), then run the command again. macOS applies the change to programs started afterwards. Run from Terminal, macOS may list Terminal instead of Kropa: allow that one.
error-capture-screen-recording-stale = macOS shows the Screen Recording permission as granted but refuses the capture. This happens when Kropa was replaced by a build with another signature. Open System Settings > Privacy & Security > Screen & System Audio Recording, remove Kropa with "-", add it again with "+", then run the command again.
error-capture-macos-too-old = Kropa needs macOS 15 (Sequoia) or later.
error-capture-no-graphical-session = No graphical session to capture: this program does not run in a macOS login session (ssh, background service). Run kropa from a terminal on the Mac itself, logged in.
error-capture-output-not-found = Screen not found: { $detail }. Run 'kropa doctor' to list the screens, then give a name or an id from that list.
error-capture-screencapturekit-error = ScreenCaptureKit failed: { $detail }
error-capture-screencapturekit-no-response = ScreenCaptureKit did not respond in time. A macOS dialog may be waiting for your reply: if so, reply to it and run the command again.
error-capture-image-encoding-failed = The image could not be encoded as a PNG file: { $detail }
error-write = Cannot write { $path }: { $detail }

## kropa doctor

doctor-title = Kropa diagnostic
doctor-section-system = System
doctor-system = { $platform } ({ $arch }), kropa { $version }
doctor-section-capabilities = Capabilities
doctor-capability = { $feature }: { $status }
doctor-why = Why: { $reason }
doctor-instead = Instead: { $fallback }
doctor-feature-capture-screen = screen capture (screen)
doctor-feature-capture-output = single-screen capture (output)
doctor-status-available = available
doctor-status-degraded = degraded
doctor-status-unavailable = unavailable
doctor-reason-no-session-bus = no D-Bus session bus: run kropa inside a graphical session
doctor-reason-portal-unavailable = the screenshot portal is missing: install xdg-desktop-portal and the backend of your desktop
doctor-reason-output-not-isolable = the portal returns all screens as one image
doctor-reason-no-graphical-session = no macOS login session: run kropa from a terminal on the Mac itself
doctor-reason-screen-recording-denied = the Screen Recording permission is missing: System Settings > Privacy & Security > Screen & System Audio Recording
doctor-reason-macos-too-old = macOS 15 or later is required
doctor-fallback-capture-screen-instead = use 'kropa capture screen' (all screens in one image)
doctor-section-details = Details
doctor-fact = { $label }: { $value }
doctor-value-yes = yes
doctor-value-no = no
doctor-value-absent = not found
doctor-value-not-probed = not checked yet
doctor-value-unreachable = not checked: no session bus
doctor-fact-desktop = Desktop (information only, never used to choose a backend)
doctor-fact-session-type = Session type
doctor-fact-wayland-display = WAYLAND_DISPLAY set
doctor-fact-x11-display = DISPLAY set
doctor-fact-session-bus = D-Bus session bus reachable
doctor-fact-session-bus-error = Session bus error
doctor-fact-portal-screenshot = Screenshot portal, interface version
doctor-fact-portal-screenshot-targets = Screenshot portal targets
doctor-fact-portal-screencast = ScreenCast portal, interface version
doctor-fact-portal-global-shortcuts = GlobalShortcuts portal, interface version
doctor-fact-portal-secret = Secret portal, interface version
doctor-fact-portal-registry = Host application registry, interface version
doctor-fact-kwin-service = KWin on the session bus (its ScreenShot2 interface is not used)
doctor-fact-app-id = Application ID
doctor-fact-desktop-file = Desktop file installed for the application ID (scripts/dev-install.sh)
doctor-fact-layer-shell = Wayland layer-shell
doctor-fact-data-control = Wayland data-control (clipboard)
doctor-fact-macos-version = macOS version
doctor-fact-graphical-session = Login session with a window server
doctor-fact-screen-recording = Screen Recording permission granted (read without asking)
doctor-fact-app-bundle = Running from an .app bundle
doctor-fact-bundle-identifier = Bundle identifier
doctor-fact-display-primary = Screen (primary, used by 'capture output' without a name)
doctor-fact-display = Screen
doctor-section-paths = Paths
doctor-path-config = Configuration file
doctor-path-config-error = Configuration error
doctor-path-captures = Captures folder
doctor-section-secrets = Secret store
doctor-secrets-store = Store: { $store } ({ $state })
doctor-secrets-profiles = Profiles holding secrets: { $count }
doctor-secrets-detail = Technical detail (not translated): { $detail }
doctor-secrets-duplicates = The same secret is stored more than once: remove the extra entries with your keyring manager.
doctor-secrets-orphans = Profiles in the store that no destination declares (leftovers, names only): { $names }. Remove their entries with your keyring manager.
doctor-secrets-orphans-unknown = Leftovers not checked: the destinations file cannot be read.
doctor-section-ipc = Application (IPC)
doctor-ipc-socket = Socket: { $path }
doctor-ipc-state = State: { $state }
doctor-ipc-state-absent = not running
doctor-ipc-state-listening = listening
doctor-ipc-state-orphan = orphan socket (the application is gone)
doctor-ipc-state-unsafe = not safe, Kropa will not use it
doctor-ipc-state-incompatible = answers, but not in a way this kropa understands
doctor-ipc-reason = Reason: { $reason }
doctor-ipc-reason-no-runtime-dir = no runtime directory (XDG_RUNTIME_DIR is not set; there is no fallback to /tmp)
doctor-ipc-reason-unsafe-runtime-dir = the runtime directory is a link, belongs to someone else or is open to other users
doctor-ipc-reason-peer-uid-mismatch = the socket belongs to another user
doctor-ipc-reason-ipc-version-mismatch = the application speaks another IPC protocol
doctor-ipc-reason-ipc-protocol-error = the answer to hello is invalid
doctor-ipc-server = Application: { $server } { $version } (IPC protocol { $protocol })
doctor-ipc-service-file = D-Bus service file installed: { $value }
doctor-ipc-service-file-unused = D-Bus service file: not used on this system
doctor-section-pending = Not available yet
doctor-pending-outputs = Screens and their names: lot 2.5

## Screenshot permission reset ('kropa doctor --reset-screenshot-permission')

doctor-reset-removed = Screenshot permission forgotten: the next capture asks again.
doctor-reset-absent = Nothing to reset: the system holds no stored answer to the screenshot request for Kropa.
error-reset-unsupported = Kropa cannot reset the screenshot permission on this system. On macOS, open System Settings > Privacy & Security > Screen & System Audio Recording, switch Kropa off and on again (or remove it with "-" and add it again with "+").
error-reset-store-unreachable = The permission store of the desktop portals could not be reached ({ $detail }). Run kropa from a terminal inside your desktop session and check that xdg-desktop-portal is installed and running; otherwise reset the permission in the privacy settings of your desktop.
error-reset-store-error = The permission store of the desktop portals refused the reset: { $detail }

## Upload and destinations (lot 1.7)

arg-expect-host = Host the file must target; required with --yes when there is no terminal
val-expect-host = HOST
err-host = expected a host name without scheme or path, such as z.example.org

error-cancelled = Canceled.
error-import-host-required = Importing without a terminal needs --yes and --expect-host HOST, so that a file cannot send you to a host you did not choose.
error-import-file-type = Unknown file type: expected a .sxcu or .toml file.
error-dest-exists = A destination named '{ $id }' already exists, or the secret store still holds entries for that name. Choose another identifier with --id.
error-dest-no-file = Cannot read the file: { $detail }

error-upload-invalid-destination = Invalid destination: { $detail }
error-upload-unknown-destination = { $detail }. List them with 'kropa dest list'.
error-upload-import-invalid = Cannot import: { $detail }
error-upload-import-unsupported = Cannot import: { $detail }
error-upload-import-too-large = Cannot import: the file is larger than 64 KiB.
error-upload-missing-variable = The destination needs a value for '{ $detail }': pass --var { $detail }=VALUE.
error-upload-invalid-variable = The value given for '{ $detail }' is not one of the choices of the destination.
error-upload-secret-missing = The secret '{ $detail }' of the destination is not set. Store it with 'kropa secrets set'.
error-upload-secret-store-not-delivered = This build has no secret store yet: on macOS it arrives with lot 1.8m (the Keychain broker). Nothing was done.
error-upload-secret-store-error = The secret store failed: { $detail }
error-upload-timeout = The server did not respond in time. The file is kept: try again in a moment.
error-upload-connect = Cannot connect to the server. Check the address and the network. The file is kept.
error-upload-tls = The secure connection failed: the server's certificate could not be verified. If it uses a private authority, add it to the system certificate store. The file is kept.
error-upload-network = The request failed. Check the network and try again. The file is kept.
error-upload-http-status = The server responded with status { $status }: { $detail }
error-upload-http-status-bare = The server responded with status { $status }.
error-upload-url-missing = The server responded, but its response contains no URL. Check the response template of the destination.
error-upload-url-invalid = The server returned a URL that cannot be used (not http or https, with control characters, or longer than 2048 bytes). Nothing was copied. The file may have been uploaded: check on the server.
error-upload-response-too-large = The server's response is larger than 1 MiB: it was not read.
error-upload-bad-response = The server's response cannot be read: { $detail }
error-upload-file-too-large = The file is larger than the destination's limit (max_size). Make it smaller, or raise that limit in the destination.
error-upload-origin-not-allowed = Refused: { $detail }. Nothing was sent.
error-upload-placement-not-allowed = Refused: { $detail }. Nothing was sent.
error-upload-secret-in-output = Refused: the server's response contains the secret, so its link was withheld and nothing was copied. The file may have been uploaded.
error-upload-https-required = Refused: { $detail }. Nothing was sent.
error-upload-cross-origin-redirect = Refused: the server redirected to another origin; the request was not repeated there.
error-upload-secret-in-argument = Refused: { $detail }
error-upload-secret-too-short = Refused: a secret must be at least 4 bytes long.
error-upload-secret-in-path = Refused: { $detail }
error-upload-import-host-mismatch = Refused: the file targets { $detail }, not the host you expected. Nothing was imported.
error-upload-io = { $detail }

warn-secret-short = The secret '{ $detail }' is short: it may be masked in more places than needed.
warn-secret-echoed = The server repeated a secret in its response: the text was masked.
warn-uploaded-link-withheld = The file may have been uploaded, but its link is withheld because it contained a secret.
warn-thumbnail-url-invalid = The thumbnail URL in the response cannot be used and was left out.
warn-deletion-url-invalid = The deletion URL in the response cannot be used and was left out.
warn-metadata-not-stripped = Metadata is not removed from the uploaded file yet (lot 4.8).
warn-clipboard-unavailable = { $what ->
    [image] Image not copied: { $reason } The image file is saved, see above.
   *[link] Link not copied: { $reason } The link is printed above.
  }
warn-clipboard-failed = { $what ->
    [image] Image not copied: { $reason } Technical detail (not translated): { $detail }
   *[link] Link not copied: { $reason } Technical detail (not translated): { $detail }
  }
warn-clipboard-failed-bare = { $what ->
    [image] Image not copied: { $reason }
   *[link] Link not copied: { $reason }
  }
note-clipboard-copied = { $what ->
    [image] Image copied
   *[link] Link copied ({ $format })
  }
clipboard-reason-no-data-control = This Wayland session does not allow clipboard access without focus (GNOME does not).
clipboard-reason-no-graphical-session = There is no graphical session to copy to.
clipboard-reason-x11-not-supported = The X11 clipboard is not supported yet; Wayland sessions with data-control are.
clipboard-reason-unsupported-platform = There is no clipboard support on this system.
clipboard-reason-connection-failed = The Wayland compositor could not be reached.
clipboard-reason-no-seat = The compositor has no seat to copy to.
clipboard-reason-compositor-error = The compositor refused the copy.
clipboard-reason-helper-spawn-failed = The process that serves the clipboard could not be started.
clipboard-reason-helper-failed = The process that serves the clipboard failed.
clipboard-reason-timeout = The compositor did not respond in time.
clipboard-reason-pasteboard-rejected = The macOS clipboard refused the content.
clipboard-reason-selection-mismatch = The system did not keep the copied content.
clipboard-reason-too-large = The content is too large for the clipboard.

note-uploaded = Uploaded to { $profile }
note-link = Link ({ $format }): { $text }
note-no-destinations = No destination configured. Import one with 'kropa dest import FILE'.

out-dest-default-marker = (default)
out-show-title = { $id } ({ $type })
out-show-label = Label: { $label }
out-show-address = Address: { $address }
out-show-origin = Origin: { $origin }
out-show-kinds = Accepts: { $kinds }
out-show-default = Default destination: yes
out-show-request = Request: { $method } { $url }
out-show-header = Header { $name }: { $value }
out-show-query = Query parameter { $name }: { $value }
out-show-field = Field { $name }: { $value }
out-show-secret = Secret { $name }: { $state }
out-show-var = Variable { $name }
out-secret-set = set
out-secret-missing = missing
out-secret-unknown = unknown (no secret store in this build: lot 1.8m)

prompt-dest-test = Send a 1x1 test image to { $id } ({ $origin })? [y/N]
prompt-dest-import = Create the destination '{ $id }' for { $origin }? [y/N]
out-test-status = Status: { $status }
out-test-header = { $name }: { $value }
out-test-url = URL: { $url }
out-test-link = Link ({ $format }): { $text }

out-import-summary = Destination '{ $id }' for { $origin }
out-import-http-warning = This address is not https.
out-import-secret = Secret { $name } ({ $placement }): { $preview }
out-import-note-text-types-ignored = Ignored: the file also declares { $detail }, which Kropa does not send.
out-import-note-legacy-syntax = The file uses the old ShareX syntax; it was migrated.
out-import-note-query-secret = A secret ({ $detail }) goes in the query string of the address: it will show up in the logs of the server and of proxies.
out-import-note-body-template-secret = The body of the request was kept as a secret ({ $detail }) and is sent as is: its variables are not replaced.
note-import-rollback-left = Could not remove the secrets already stored for { $id }: { $names }. Remove them with 'kropa secrets remove { $id } <name>' or with your keyring manager, before importing it again.
note-import-outcome-unknown = The keyring did not respond while storing { $id }/{ $name }: it may have been stored. Check with 'kropa secrets status'.
note-imported = Imported destination '{ $id }' ({ $origin }).

## Secrets (lot 1.8)

cmd-secrets-allow = Authorize a secret for another origin or place (asks on a terminal)
arg-origin = Origin the secret may also be sent to (https://host[:port])
val-origin = URL
arg-placement = Place the secret may also be put in: header:NAME, query:KEY, form:FIELD or body
val-placement = PLACE
arg-secrets-allow-yes = Has no effect: this confirmation is never given without a terminal
err-placement = expected a place such as header:NAME, query:KEY, form:FIELD or body
error-secret-undeclared = The destination '{ $profile }' does not declare a secret named '{ $name }'. Check the name with 'kropa dest show { $profile }'.
error-secret-no-origin = The address of the destination is not a valid origin (http or https, without user:pass@).
error-secret-too-long = The secret is longer than 4 KiB.
error-secret-not-text = The secret is not valid UTF-8 text.
error-secret-unreadable = Cannot read the secret from stdin or from the given file descriptor.
error-secrets-no-body = The destination has no request body to attach a secret to.
error-secrets-bad-placement = Refused: a secret may only be placed in header:NAME, query:KEY, form:FIELD or body, never in the host or the path of the address.
error-secrets-bad-origin = Not a valid origin: expected an http:// or https:// address without user:pass@.
error-allow-interactive = Authorizing a secret for a new origin or place needs a terminal and is never confirmed by --yes. Run the command in a terminal.
error-ipc-no-runtime-dir = The { $app } application cannot be reached: there is no safe runtime directory. Kropa never falls back to /tmp; set XDG_RUNTIME_DIR (log in through your desktop session). { $detail }
error-ipc-unsafe-runtime-dir = The { $app } application was not contacted: its directory is not safe ({ $detail }).
error-ipc-daemon-not-running = The { $app } application is not running ({ $detail }).
error-ipc-daemon-start-failed = The { $app } application could not be started ({ $detail }).
error-ipc-daemon-start-timeout = The { $app } application was started but did not answer in time ({ $detail }).
error-ipc-peer-uid-mismatch = Nothing was sent: the socket of the { $app } application belongs to another user ({ $detail }).
error-ipc-ipc-version-mismatch = This kropa and the { $app } application do not speak the same IPC protocol ({ $detail }). Update both to the same version.
error-ipc-ipc-protocol-error = The { $app } application did not answer as expected ({ $detail }).
error-ipc-rpc = { $detail }
error-hardening-failed = Stopped for safety: memory dumps are allowed and setting { $step } failed ({ $detail }). Kropa does not run in that state.
error-upload-keyring-locked = The system keyring is locked and cannot be unlocked from here ({ $detail }). Unlock it (log in to your desktop session, or open your keyring manager) and try again.
error-upload-keyring-timeout = The system keyring did not respond in time ({ $detail }). Nothing was changed by this command. Try again.
error-upload-keyring-timeout-write = The system keyring did not respond in time ({ $detail }). The result is unknown: the change may or may not have been made. Check with 'kropa secrets status'.
error-upload-keyring-ambiguous = The system keyring holds the same secret more than once ({ $detail }). Remove the extra entries with your keyring manager (look for 'io.github.fs0ciety7000.Kropa').
error-upload-keyring-unavailable = No usable system keyring ({ $detail }). The 'age' vault fallback (kropa secrets use vault) arrives with lot 2.8; nothing is switched by itself.
error-upload-keychain-denied = Access to the Keychain was refused ({ $detail }). Nothing was done. Run the command again and choose Always Allow (or Allow) when macOS asks whether Kropa may use its item, or change it in Keychain Access (item 'io.github.fs0ciety7000.Kropa', tab Access Control).
error-upload-keychain-broker-refused = The Keychain broker refused this caller ({ $detail }). Only the signed Kropa app may use it; the kropa command reads the Keychain by itself.
error-upload-keychain-broker-unavailable = The Keychain broker cannot be used ({ $detail }). Nothing is switched by itself: use the kropa command, which reads the Keychain directly, or choose the 'age' vault (lot 2.8).
out-secrets-none = none
out-secrets-store = Secret store: { $store } ({ $state })
out-secrets-state-available = available
out-secrets-state-locked = locked
out-secrets-state-unavailable = unavailable
out-secrets-state-ambiguous = ambiguous
out-secrets-state-denied = access denied
out-secrets-state-not_delivered = not available in this build
out-secrets-detail = { $detail }
out-secrets-profile = { $profile }: entries { $entries }, still empty { $empty }
out-secrets-fallback = The 'age' vault fallback (kropa secrets use vault) arrives with lot 2.8; nothing is switched by itself.
out-secrets-new-entry = New secret { $profile }/{ $name }: where it may go is fixed now.
out-secrets-current = Secret { $profile }/{ $name } today:
out-secrets-origins = Sent only to: { $origins }
out-secrets-placements = Placed only in: { $placements }
out-secrets-query-warning = A secret in the query string of an address ends up in the logs of the server and of proxies.
out-secrets-add-origin = Adding the origin: { $origin }
out-secrets-add-placement = Adding the place: { $placement }
prompt-secret-value = Value of { $profile }/{ $name } (hidden):
prompt-secrets-set = Store the secret { $profile }/{ $name } with these limits? [y/N]
prompt-secrets-remove = Remove the secret { $profile }/{ $name }? [y/N]
prompt-secrets-allow = Allow this for the secret { $profile }/{ $name }? [y/N]
note-secret-set = Secret { $profile }/{ $name } stored.
note-secret-removed = Secret { $profile }/{ $name } removed.
note-secret-absent = There was no secret { $profile }/{ $name }.
note-secret-allowed = Secret { $profile }/{ $name } updated.

## dest add, edit, remove, set-default, export (lot 2.6)
arg-dest-edit-yes = Has no effect: moving a secret to another address or place is never confirmed without a terminal
error-dest-add-http = 'dest add --type http' does not exist: an HTTP API is described in a TOML file. Import it with 'kropa dest import FILE.toml' (a model for Imgur is in docs/examples/imgur.toml) or from a ShareX .sxcu file.
error-dest-url-required = A zipline destination needs --url.
error-dest-option-type = { $option } does not apply to a { $type } destination.
error-dest-edit-key = This key cannot be set from the command line. Keys that can be: { $keys }. Anything else is changed in the editor ('kropa dest edit { $id }' without options).
error-dest-edit-value = Not a valid value for '{ $key }': { $detail }
error-dest-edit-kinds = expected image, file or image,file
error-dest-edit-label = expected text of 1 to 80 characters, without control characters
error-dest-edit-interactive = Moving the secrets of '{ $id }' to another address or place needs a terminal and is never confirmed by --yes. Run the command in a terminal.
error-dest-edit-editor-interactive = Editing a profile in an editor needs a terminal. Use --url or --set without one.
error-dest-edit-no-editor = No editor: set the EDITOR environment variable, or change the profile with --url or --set.
error-dest-edit-editor-start = The editor could not be started: { $detail }
error-dest-edit-editor-status = The editor ended with an error: nothing was changed.
error-dest-edit-invalid = The edited profile is not valid: { $detail }
error-dest-edit-literal-secret = Refused: the edited profile holds what looks like the value of a secret. A secret never goes in the file: leave { "{secret:NAME}" } in place and store the value with 'kropa secrets set { $id } NAME'. Nothing was changed.
error-dest-export-literal-secret = Refused: the profile '{ $id }' holds what looks like the value of a secret in destinations.toml, so nothing was exported. Move it to the secret store with 'kropa secrets set { $id } NAME' and leave { "{secret:NAME}" } in the file.
error-dest-nothing-to-remove = There is no destination named '{ $id }', and the secret store holds nothing under that name.
prompt-dest-edit = Let the secrets of '{ $id }' follow these changes? [y/N]
prompt-dest-edit-again = Open the editor again to fix it? [y/N]
prompt-dest-remove = Remove the destination '{ $id }' and the secrets stored for it ({ $count })? [y/N]
out-edit-origin-change = Address of the profile: { $old } -> { $new }
out-edit-placement-change = Secret { $name }, places: { $old } -> { $new }
note-dest-added = Destination '{ $id }' added ({ $origin }).
note-dest-edited = Destination '{ $id }' updated.
note-dest-edit-unchanged = Nothing changed in '{ $id }'.
note-dest-edit-followed = The secrets of '{ $id }' may now also go to the new address and places.
note-dest-edit-authorize-failed = The destination '{ $id }' was updated, but its secret '{ $name }' could not be authorized for the new address or places ({ $detail }): the next upload will be refused. Authorize it with 'kropa secrets allow { $id } { $name } --origin URL' on a terminal.
note-dest-add-default-failed = The destination '{ $id }' was added, but it could not be made the default: { $detail }. Try 'kropa dest set-default { $id }'.
note-dest-add-rollback-left = Could not remove the secrets already stored for { $id }: { $names }. Remove them with 'kropa secrets remove { $id } <name>' or with your keyring manager, before adding it again.
note-dest-removed = Destination '{ $id }' removed.
note-dest-removed-orphans = There was no destination '{ $id }' in the configuration, but the store held secrets under that name: they were removed.
note-dest-remove-default-cleared = '{ $id }' was the default destination: there is none now.
note-dest-default-set = Default destination: '{ $id }'.
note-dest-default-cleared = No default destination any more.
