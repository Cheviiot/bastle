// SPDX-License-Identifier: GPL-3.0-or-later

use std::{error::Error, fmt};

use ashpd::{
    desktop::{
        dynamic_launcher::{DynamicLauncherProxy, LauncherType},
        file_chooser::FileChooserProxy,
        ResponseError,
    },
    documents::Documents,
    PortalError,
};
use async_trait::async_trait;
use gettextrs::gettext;
use gtk::glib;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalFailureKind {
    Unavailable,
    Unsupported,
    Cancelled,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalOperationError {
    pub kind: PortalFailureKind,
    operation: String,
    detail: String,
}

impl PortalOperationError {
    pub fn new(
        kind: PortalFailureKind,
        operation: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation: operation.into(),
            detail: detail.into(),
        }
    }

    pub fn from_ashpd(operation: impl Into<String>, error: ashpd::Error) -> Self {
        let kind = classify_ashpd_error(&error);
        Self::new(kind, operation, error.to_string())
    }

    fn kind_label(&self) -> String {
        match self.kind {
            PortalFailureKind::Unavailable => gettext("portal interface unavailable"),
            PortalFailureKind::Unsupported => gettext("portal capability unsupported"),
            PortalFailureKind::Cancelled => gettext("portal request cancelled"),
            PortalFailureKind::Denied => gettext("portal request denied"),
            PortalFailureKind::Failed => gettext("portal request failed"),
        }
    }
}

impl fmt::Display for PortalOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} ({})",
            self.operation,
            self.kind_label(),
            self.detail
        )
    }
}

impl Error for PortalOperationError {}

pub fn classify_ashpd_error(error: &ashpd::Error) -> PortalFailureKind {
    match error {
        ashpd::Error::PortalNotFound(_) => PortalFailureKind::Unavailable,
        ashpd::Error::RequiresVersion(_, _) => PortalFailureKind::Unsupported,
        ashpd::Error::Response(ResponseError::Cancelled)
        | ashpd::Error::Portal(PortalError::Cancelled(_)) => PortalFailureKind::Cancelled,
        ashpd::Error::Portal(PortalError::NotAllowed(_)) => PortalFailureKind::Denied,
        ashpd::Error::Response(ResponseError::Other) => PortalFailureKind::Failed,
        ashpd::Error::Zbus(error) if dbus_interface_is_missing(&error.to_string()) => {
            PortalFailureKind::Unavailable
        }
        _ => PortalFailureKind::Failed,
    }
}

fn dbus_interface_is_missing(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("unknowninterface")
        || message.contains("serviceunknown")
        || message.contains("namehasnoowner")
        || message.contains("not found")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalFeature {
    Available { version: u32 },
    Problem(PortalOperationError),
}

impl PortalFeature {
    fn from_result(result: Result<u32, PortalOperationError>) -> Self {
        match result {
            Ok(version) => Self::Available { version },
            Err(error) => Self::Problem(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicLauncherCapability {
    pub interface: PortalFeature,
    pub application_launchers: Option<bool>,
    pub web_application_launchers: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalCapabilities {
    pub desktop: String,
    pub dynamic_launcher: DynamicLauncherCapability,
    pub file_chooser: PortalFeature,
    pub documents: PortalFeature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicLauncherProbe {
    pub version: u32,
    pub application_launchers: bool,
    pub web_application_launchers: bool,
}

#[async_trait(?Send)]
pub trait PortalProbeBackend {
    async fn dynamic_launcher(&self) -> Result<DynamicLauncherProbe, PortalOperationError>;
    async fn file_chooser(&self) -> Result<u32, PortalOperationError>;
    async fn documents(&self) -> Result<u32, PortalOperationError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AshpdPortalProbe;

#[async_trait(?Send)]
impl PortalProbeBackend for AshpdPortalProbe {
    async fn dynamic_launcher(&self) -> Result<DynamicLauncherProbe, PortalOperationError> {
        let proxy = DynamicLauncherProxy::new().await.map_err(|error| {
            PortalOperationError::from_ashpd(gettext("Dynamic Launcher"), error)
        })?;
        let supported = proxy.supported_launcher_types().await.map_err(|error| {
            PortalOperationError::from_ashpd(gettext("Dynamic Launcher capabilities"), error)
        })?;
        Ok(DynamicLauncherProbe {
            version: proxy.version(),
            application_launchers: supported.contains(LauncherType::Application),
            web_application_launchers: supported.contains(LauncherType::WebApplication),
        })
    }

    async fn file_chooser(&self) -> Result<u32, PortalOperationError> {
        FileChooserProxy::new()
            .await
            .map(|proxy| proxy.version())
            .map_err(|error| PortalOperationError::from_ashpd(gettext("File Chooser"), error))
    }

    async fn documents(&self) -> Result<u32, PortalOperationError> {
        Documents::new()
            .await
            .map(|proxy| proxy.version())
            .map_err(|error| PortalOperationError::from_ashpd(gettext("Documents"), error))
    }
}

pub async fn probe_capabilities() -> PortalCapabilities {
    probe_capabilities_with(&AshpdPortalProbe).await
}

async fn probe_capabilities_with(backend: &impl PortalProbeBackend) -> PortalCapabilities {
    let dynamic_launcher = match backend.dynamic_launcher().await {
        Ok(capability) => DynamicLauncherCapability {
            interface: PortalFeature::Available {
                version: capability.version,
            },
            application_launchers: Some(capability.application_launchers),
            web_application_launchers: Some(capability.web_application_launchers),
        },
        Err(error) => DynamicLauncherCapability {
            interface: PortalFeature::Problem(error),
            application_launchers: None,
            web_application_launchers: None,
        },
    };
    PortalCapabilities {
        desktop: current_desktop(),
        dynamic_launcher,
        file_chooser: PortalFeature::from_result(backend.file_chooser().await),
        documents: PortalFeature::from_result(backend.documents().await),
    }
}

pub fn current_desktop() -> String {
    desktop_name(std::env::var_os("XDG_CURRENT_DESKTOP"))
}

fn desktop_name(value: Option<std::ffi::OsString>) -> String {
    value
        .map(|desktop| desktop.to_string_lossy().into_owned())
        .filter(|desktop| !desktop.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

pub fn classify_file_dialog_error(
    operation: impl Into<String>,
    error: &glib::Error,
) -> Option<PortalOperationError> {
    if error.matches(gtk::DialogError::Cancelled) || error.matches(gtk::DialogError::Dismissed) {
        return None;
    }
    let detail = error.to_string();
    let normalized = detail.to_ascii_lowercase();
    let kind = if dbus_interface_is_missing(&normalized) || normalized.contains("unavailable") {
        PortalFailureKind::Unavailable
    } else if normalized.contains("denied")
        || normalized.contains("not allowed")
        || normalized.contains("permission")
    {
        PortalFailureKind::Denied
    } else {
        PortalFailureKind::Failed
    };
    Some(PortalOperationError::new(kind, operation, detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct FakeProbe {
        dynamic: Result<DynamicLauncherProbe, PortalOperationError>,
        file_chooser: Result<u32, PortalOperationError>,
        documents: Result<u32, PortalOperationError>,
    }

    #[async_trait(?Send)]
    impl PortalProbeBackend for FakeProbe {
        async fn dynamic_launcher(&self) -> Result<DynamicLauncherProbe, PortalOperationError> {
            self.dynamic.clone()
        }

        async fn file_chooser(&self) -> Result<u32, PortalOperationError> {
            self.file_chooser.clone()
        }

        async fn documents(&self) -> Result<u32, PortalOperationError> {
            self.documents.clone()
        }
    }

    fn failure(kind: PortalFailureKind) -> PortalOperationError {
        PortalOperationError::new(kind, "test", "injected failure")
    }

    fn successful_probe() -> FakeProbe {
        FakeProbe {
            dynamic: Ok(DynamicLauncherProbe {
                version: 1,
                application_launchers: true,
                web_application_launchers: false,
            }),
            file_chooser: Ok(4),
            documents: Ok(3),
        }
    }

    #[test]
    fn fake_probe_reports_success_and_launcher_types() {
        let report = futures::executor::block_on(probe_capabilities_with(&successful_probe()));
        assert_eq!(
            report.dynamic_launcher.interface,
            PortalFeature::Available { version: 1 }
        );
        assert_eq!(report.dynamic_launcher.application_launchers, Some(true));
        assert_eq!(
            report.dynamic_launcher.web_application_launchers,
            Some(false)
        );
        assert_eq!(report.file_chooser, PortalFeature::Available { version: 4 });
        assert_eq!(report.documents, PortalFeature::Available { version: 3 });
    }

    #[test]
    fn fake_probe_keeps_missing_interfaces_independent() {
        let mut probe = successful_probe();
        probe.file_chooser = Err(failure(PortalFailureKind::Unavailable));
        let report = futures::executor::block_on(probe_capabilities_with(&probe));
        assert!(matches!(
            report.file_chooser,
            PortalFeature::Problem(PortalOperationError {
                kind: PortalFailureKind::Unavailable,
                ..
            })
        ));
        assert_eq!(report.documents, PortalFeature::Available { version: 3 });
    }

    #[test]
    fn fake_probe_reports_unsupported_application_launchers() {
        let mut probe = successful_probe();
        probe.dynamic = Ok(DynamicLauncherProbe {
            version: 1,
            application_launchers: false,
            web_application_launchers: true,
        });
        let report = futures::executor::block_on(probe_capabilities_with(&probe));
        assert_eq!(report.dynamic_launcher.application_launchers, Some(false));
        assert_eq!(
            report.dynamic_launcher.web_application_launchers,
            Some(true)
        );
    }

    #[test]
    fn fake_probe_preserves_cancellation() {
        let mut probe = successful_probe();
        probe.documents = Err(failure(PortalFailureKind::Cancelled));
        let report = futures::executor::block_on(probe_capabilities_with(&probe));
        assert!(matches!(
            report.documents,
            PortalFeature::Problem(PortalOperationError {
                kind: PortalFailureKind::Cancelled,
                ..
            })
        ));
    }

    #[test]
    fn fake_probe_preserves_denial() {
        let mut probe = successful_probe();
        probe.dynamic = Err(failure(PortalFailureKind::Denied));
        let report = futures::executor::block_on(probe_capabilities_with(&probe));
        assert!(matches!(
            report.dynamic_launcher.interface,
            PortalFeature::Problem(PortalOperationError {
                kind: PortalFailureKind::Denied,
                ..
            })
        ));
    }

    #[test]
    fn cancelled_file_dialog_is_not_an_error() {
        let error = glib::Error::new(gtk::DialogError::Cancelled, "cancelled");
        assert!(classify_file_dialog_error("restore", &error).is_none());
    }

    #[test]
    fn failed_file_dialog_remains_actionable() {
        let error = glib::Error::new(gtk::DialogError::Failed, "Permission denied");
        assert_eq!(
            classify_file_dialog_error("restore", &error)
                .expect("failure")
                .kind,
            PortalFailureKind::Denied
        );
    }

    #[test]
    fn catch_all_response_is_failure_not_denial() {
        assert_eq!(
            classify_ashpd_error(&ashpd::Error::Response(ResponseError::Other)),
            PortalFailureKind::Failed
        );
        assert_eq!(
            classify_ashpd_error(&ashpd::Error::Portal(PortalError::NotAllowed(
                "policy denied the request".to_owned(),
            ))),
            PortalFailureKind::Denied
        );
    }

    #[test]
    fn missing_desktop_session_has_a_stable_diagnostic() {
        assert_eq!(desktop_name(None), "unknown");
        assert_eq!(desktop_name(Some("KDE".into())), "KDE");
        assert_eq!(desktop_name(Some("  ".into())), "unknown");
    }
}
