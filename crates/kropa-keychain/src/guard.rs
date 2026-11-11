// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks the broker makes **before any call to the Keychain** (`docs/security/secrets.md`
//! section 2.2.1, point 2), and the refusals.
//!
//! The facts the checks rest on come from a [`LaunchFacts`], so that the order and the outcome of
//! every check are tested here without a Mac, a signature or a Keychain. The real facts are
//! [`crate::SystemFacts`].
//!
//! What a check proves, said exactly (the section 7 limits apply):
//!
//! - *parent is not launchd*, *the channel is a connected `AF_UNIX` stream socket*: the broker was
//!   started by a process that made a socket pair for it, not from a shell or by launchd;
//! - *the peer's audit token names the parent*: the end of the socket we hold was made by our
//!   parent (macOS: `LOCAL_PEERTOKEN`);
//! - *the peer's code satisfies the requirement*: the process that holds the other end is signed
//!   with the project certificate under the identifier of `kropa-app`, with the hardened runtime,
//!   so another process of the user cannot get into it (macOS: `SecCode`);
//! - *the broker is signed with the project certificate itself*: the requirement is derived from
//!   the broker's own signature. A build signed ad hoc or not at all **refuses everything**: there
//!   is no weaker mode, because an ad hoc identifier can be given to any binary.
//!
//! Not proved: that the peer is not compromised, that it did not `exec` another program after it
//! made the socket pair (to be tried on a Mac: `docs/testing/manual-macos.md`), anything about a
//! process of the same user that can write to the application bundle.

/// What fd 3 turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState {
    /// Nothing is open at fd 3.
    Missing,
    /// Open, but not a connected `AF_UNIX` stream socket (a file, a pipe, a terminal...).
    NotAConnectedUnixStream,
    /// A connected `AF_UNIX` stream socket.
    Connected,
}

/// The audit token of the process at the other end of the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerToken {
    /// The process identifier the token names.
    pub pid: u32,
    /// The raw `audit_token_t` (eight 32-bit words), handed back to the code signing services.
    pub raw: [u32; 8],
}

/// The leaf certificate of the project, as the 40 hexadecimal digits of its SHA-1 that a code
/// requirement (`certificate leaf = H"..."`) names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCertificate(String);

impl ProjectCertificate {
    /// 40 hexadecimal digits (any case); `None` otherwise.
    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        (hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Self(hex.to_ascii_lowercase()))
    }

    /// The digits, in lower case.
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.0
    }
}

/// The code-signing identifier of `kropa` (the broker): the application ID, which `codesign` gives
/// the bundle and its main executable (`scripts/bundle-macos.sh`).
pub const BROKER_IDENTIFIER: &str = "io.github.fs0ciety7000.Kropa";

/// The code-signing identifier of `kropa-app` (the client of the broker), signed on its own by
/// `scripts/bundle-macos.sh`.
pub const CLIENT_IDENTIFIER: &str = "io.github.fs0ciety7000.Kropa.app";

/// The code requirement a peer must satisfy: this identifier, and the leaf certificate of the
/// project.
fn requirement(identifier: &str, certificate: &ProjectCertificate) -> String {
    format!(
        "identifier \"{identifier}\" and certificate leaf = H\"{}\"",
        certificate.hex()
    )
}

/// The requirement `kropa-app` must satisfy to be served.
#[must_use]
pub fn client_requirement(certificate: &ProjectCertificate) -> String {
    requirement(CLIENT_IDENTIFIER, certificate)
}

/// The requirement the broker binary must satisfy for the client to start it.
#[must_use]
pub fn broker_requirement(certificate: &ProjectCertificate) -> String {
    requirement(BROKER_IDENTIFIER, certificate)
}

/// The leaf certificate named by a designated requirement of the form
/// `... certificate leaf = H"<40 hex digits>" ...`, which is what a signature made with a
/// certificate that no anchor vouches for (the self-signed certificate of the project) gives. An ad
/// hoc signature (`cdhash H"..."`) and any other form give `None`.
#[must_use]
pub fn certificate_from_requirement(text: &str) -> Option<ProjectCertificate> {
    const MARK: &str = "certificate leaf = H\"";
    let start = text.find(MARK)? + MARK.len();
    let digits = &text[start..];
    let end = digits.find('"')?;
    ProjectCertificate::from_hex(&digits[..end])
}

/// A fact that could not be established; the text is fixed and never holds data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Unverified(pub &'static str);

/// What the broker may ask about how it was launched.
pub trait LaunchFacts {
    /// `getppid()`.
    fn parent_pid(&self) -> u32;

    /// What fd 3 is.
    fn channel(&self) -> ChannelState;

    /// The audit token of the peer of the channel (`LOCAL_PEERTOKEN`).
    ///
    /// # Errors
    /// [`Unverified`] when the platform cannot give it.
    fn peer_token(&self) -> Result<PeerToken, Unverified>;

    /// The project certificate the broker itself is signed with.
    ///
    /// # Errors
    /// [`Unverified`] when the broker is not signed with a certificate (ad hoc, unsigned), or the
    /// platform has no code signature to look at.
    fn project_certificate(&self) -> Result<ProjectCertificate, Unverified>;

    /// Whether the code behind `token` satisfies the requirement of `kropa-app` for `certificate`.
    ///
    /// # Errors
    /// [`Unverified`] when it does not, or when this cannot be checked.
    fn check_peer_code(
        &self,
        token: &PeerToken,
        certificate: &ProjectCertificate,
    ) -> Result<(), Unverified>;
}

/// Why the broker refused to serve. Each has a stable exit status, which is all a client sees of
/// it (the broker's own stderr is `/dev/null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// The parent is launchd, or gone.
    #[error("parent_is_launchd")]
    ParentIsLaunchd,
    /// fd 3 is not a connected socket.
    #[error("no_channel")]
    NoChannel,
    /// The audit token of the peer cannot be read.
    #[error("peer_token_unavailable")]
    PeerTokenUnavailable,
    /// The peer of the channel is not the parent.
    #[error("peer_is_not_parent")]
    PeerNotParent,
    /// The broker is not signed with the project certificate (ad hoc, unsigned, other).
    #[error("broker_not_project_signed")]
    BrokerNotProjectSigned,
    /// The peer's code does not satisfy the requirement.
    #[error("peer_code_rejected")]
    PeerCodeRejected,
    /// The request is not exactly one valid request.
    #[error("bad_request")]
    BadRequest,
    /// A valid request for an operation that does not exist.
    #[error("unknown_operation")]
    UnknownOperation,
    /// The parent ended before the broker could act or answer.
    #[error("parent_gone")]
    ParentGone,
    /// The whole call took more than the broker allows (120 s, prompt included).
    #[error("timeout")]
    Timeout,
    /// The channel failed.
    #[error("channel_io")]
    ChannelIo,
}

impl Refusal {
    const ALL: [Self; 11] = [
        Self::ParentIsLaunchd,
        Self::NoChannel,
        Self::PeerTokenUnavailable,
        Self::PeerNotParent,
        Self::BrokerNotProjectSigned,
        Self::PeerCodeRejected,
        Self::BadRequest,
        Self::UnknownOperation,
        Self::ParentGone,
        Self::Timeout,
        Self::ChannelIo,
    ];

    /// The exit status of the broker for this refusal (70 to 80; 0 is success).
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Self::ParentIsLaunchd => 70,
            Self::NoChannel => 71,
            Self::PeerTokenUnavailable => 72,
            Self::PeerNotParent => 73,
            Self::BrokerNotProjectSigned => 74,
            Self::PeerCodeRejected => 75,
            Self::BadRequest => 76,
            Self::UnknownOperation => 77,
            Self::ParentGone => 78,
            Self::Timeout => 79,
            Self::ChannelIo => 80,
        }
    }

    /// The refusal a broker's exit status stands for.
    #[must_use]
    pub fn from_exit_code(code: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|r| i32::from(r.exit_code()) == code)
    }

    /// The machine name of the refusal (`Display` gives the same).
    #[must_use]
    pub fn reason(self) -> String {
        self.to_string()
    }
}

/// What a launch that passed the checks knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    /// The pid of the parent at the time of the checks; it must still be the parent when the
    /// broker acts and when it answers.
    pub parent: u32,
}

/// Runs the checks of section 2.2.1 point 2 in order; the first that fails refuses, and nothing
/// after it is looked at.
///
/// # Errors
/// [`Refusal`] for the first check that fails.
pub fn check_launch(facts: &dyn LaunchFacts) -> Result<Admitted, Refusal> {
    let parent = facts.parent_pid();
    if parent <= 1 {
        return Err(Refusal::ParentIsLaunchd);
    }
    if facts.channel() != ChannelState::Connected {
        return Err(Refusal::NoChannel);
    }
    let token = facts
        .peer_token()
        .map_err(|_| Refusal::PeerTokenUnavailable)?;
    if token.pid != parent {
        return Err(Refusal::PeerNotParent);
    }
    // The requirement for the peer is derived from the broker's own signature, so this comes first.
    let certificate = facts
        .project_certificate()
        .map_err(|_| Refusal::BrokerNotProjectSigned)?;
    facts
        .check_peer_code(&token, &certificate)
        .map_err(|_| Refusal::PeerCodeRejected)?;
    Ok(Admitted { parent })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{
        ChannelState, LaunchFacts, PeerToken, ProjectCertificate, Refusal, Unverified,
        check_launch,
    };

    /// Facts that say what the test tells them, and note what was asked of them.
    struct Fake {
        parent: u32,
        channel: ChannelState,
        token: Result<PeerToken, Unverified>,
        certificate: Result<ProjectCertificate, Unverified>,
        peer_code: Result<(), Unverified>,
        asked: RefCell<Vec<&'static str>>,
    }

    fn token(pid: u32) -> PeerToken {
        PeerToken { pid, raw: [0; 8] }
    }

    impl Fake {
        fn good() -> Self {
            Self {
                parent: 500,
                channel: ChannelState::Connected,
                token: Ok(token(500)),
                certificate: Ok(ProjectCertificate::from_hex(&"ab".repeat(20)).unwrap()),
                peer_code: Ok(()),
                asked: RefCell::new(Vec::new()),
            }
        }

        fn asked(&self) -> Vec<&'static str> {
            self.asked.borrow().clone()
        }
    }

    impl LaunchFacts for Fake {
        fn parent_pid(&self) -> u32 {
            self.asked.borrow_mut().push("parent");
            self.parent
        }

        fn channel(&self) -> ChannelState {
            self.asked.borrow_mut().push("channel");
            self.channel
        }

        fn peer_token(&self) -> Result<PeerToken, Unverified> {
            self.asked.borrow_mut().push("token");
            self.token
        }

        fn project_certificate(&self) -> Result<ProjectCertificate, Unverified> {
            self.asked.borrow_mut().push("certificate");
            self.certificate.clone()
        }

        fn check_peer_code(
            &self,
            _token: &PeerToken,
            _certificate: &ProjectCertificate,
        ) -> Result<(), Unverified> {
            self.asked.borrow_mut().push("peer_code");
            self.peer_code
        }
    }

    #[test]
    fn a_launch_that_passes_every_check_is_admitted_in_the_order_of_the_spec() {
        let facts = Fake::good();
        let admitted = check_launch(&facts).unwrap();
        assert_eq!(admitted.parent, 500);
        assert_eq!(
            facts.asked(),
            ["parent", "channel", "token", "certificate", "peer_code"]
        );
    }

    #[test]
    fn a_parent_that_is_launchd_or_gone_is_refused_before_anything_else_is_looked_at() {
        for parent in [0, 1] {
            let facts = Fake {
                parent,
                ..Fake::good()
            };
            assert_eq!(check_launch(&facts), Err(Refusal::ParentIsLaunchd));
            assert_eq!(facts.asked(), ["parent"]);
        }
    }

    #[test]
    fn a_channel_that_is_not_a_connected_socket_is_refused() {
        for channel in [ChannelState::Missing, ChannelState::NotAConnectedUnixStream] {
            let facts = Fake {
                channel,
                ..Fake::good()
            };
            assert_eq!(check_launch(&facts), Err(Refusal::NoChannel));
            assert_eq!(facts.asked(), ["parent", "channel"]);
        }
    }

    #[test]
    fn a_peer_token_that_cannot_be_read_is_refused() {
        let facts = Fake {
            token: Err(Unverified("no token")),
            ..Fake::good()
        };
        assert_eq!(check_launch(&facts), Err(Refusal::PeerTokenUnavailable));
        assert_eq!(facts.asked(), ["parent", "channel", "token"]);
    }

    #[test]
    fn a_peer_that_is_not_the_parent_is_refused_before_its_code_is_looked_at() {
        let facts = Fake {
            token: Ok(token(501)),
            ..Fake::good()
        };
        assert_eq!(check_launch(&facts), Err(Refusal::PeerNotParent));
        assert!(!facts.asked().contains(&"peer_code"));
    }

    #[test]
    fn a_broker_that_is_not_signed_with_the_certificate_refuses_everything() {
        let facts = Fake {
            certificate: Err(Unverified("ad hoc")),
            ..Fake::good()
        };
        assert_eq!(check_launch(&facts), Err(Refusal::BrokerNotProjectSigned));
        assert!(!facts.asked().contains(&"peer_code"));
    }

    #[test]
    fn a_peer_whose_code_is_rejected_is_refused() {
        let facts = Fake {
            peer_code: Err(Unverified("requirement not satisfied")),
            ..Fake::good()
        };
        assert_eq!(check_launch(&facts), Err(Refusal::PeerCodeRejected));
    }

    #[test]
    fn refusals_have_distinct_exit_statuses_that_map_back() {
        let mut seen = Vec::new();
        for r in Refusal::ALL {
            let code = r.exit_code();
            assert!((70..=80).contains(&code), "{r}");
            assert!(!seen.contains(&code), "{r}");
            seen.push(code);
            assert_eq!(Refusal::from_exit_code(i32::from(code)), Some(r));
        }
        assert_eq!(Refusal::from_exit_code(0), None);
        assert_eq!(Refusal::from_exit_code(1), None);
    }

    #[test]
    fn the_certificate_is_read_from_a_designated_requirement() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        let dr = format!("identifier \"io.github.fs0ciety7000.Kropa\" and certificate leaf = H\"{hex}\"");
        assert_eq!(certificate_from_requirement(&dr).unwrap().hex(), hex);
        for other in [
            format!("cdhash H\"{hex}\""),
            "anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\"".to_owned(),
            "identifier \"x\" and certificate leaf = H\"abc\"".to_owned(),
            String::new(),
        ] {
            assert!(certificate_from_requirement(&other).is_none(), "{other}");
        }
    }

    #[test]
    fn the_requirements_name_the_right_identifier_and_the_certificate() {
        let c = ProjectCertificate::from_hex(&"ab".repeat(20)).unwrap();
        assert_eq!(
            client_requirement(&c),
            format!("identifier \"io.github.fs0ciety7000.Kropa.app\" and certificate leaf = H\"{}\"", "ab".repeat(20))
        );
        assert!(broker_requirement(&c).starts_with("identifier \"io.github.fs0ciety7000.Kropa\" and"));
        // The build script signs with these identifiers: a drift would refuse every client.
        let script = include_str!("../../../scripts/bundle-macos.sh");
        assert!(script.contains(&format!("APP_ID=\"{BROKER_IDENTIFIER}\"")));
        assert!(script.contains(&format!("{CLIENT_IDENTIFIER}\"")) || script.contains("\"$APP_ID.app\""));
    }

    #[test]
    fn a_certificate_is_forty_hex_digits() {
        assert!(ProjectCertificate::from_hex(&"AB".repeat(20)).is_some());
        assert_eq!(
            ProjectCertificate::from_hex(&"AB".repeat(20)).unwrap().hex(),
            "ab".repeat(20)
        );
        for bad in ["", "ab", &"zz".repeat(20), &"ab".repeat(21)] {
            assert!(ProjectCertificate::from_hex(bad).is_none(), "{bad}");
        }
    }
}
