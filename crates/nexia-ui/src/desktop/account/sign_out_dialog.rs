//! Confirmation dialog for signing out non-system accounts with unlink vs clean options.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use nexia_core::account::Account;

use crate::widget::FileExplorerBackend;

/// Confirmation dialog for signing out of a non-system account.
pub struct SignOutAccountDialog;

impl SignOutAccountDialog {
    /// Presents the sign out confirmation dialog.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        account: Account,
        on_success: impl Fn() + 'static,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let dialog = adw::MessageDialog::builder()
            .heading("Sign Out Account?")
            .body(format!(
                "Are you sure you want to sign out from “{}” ({})?\n\nChoose whether you want to keep your local files or remove synchronized folders from your computer.",
                account.name, account.server.server_url
            ))
            .build();

        if let Some(ref win) = root_window {
            dialog.set_transient_for(Some(win));
        }

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("unlink", "Sign Out & Keep Files");
        dialog.add_response("clean", "Sign Out & Clean Syncs");

        dialog.set_response_appearance("clean", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("unlink", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let b = backend.clone();
        let toast = toast_overlay.clone();
        let acc_id = account.id.clone();
        let acc_name = account.name.clone();
        let cb_shared = std::rc::Rc::new(on_success);

        dialog.connect_response(None, move |d, response| {
            if response == "unlink" || response == "clean" {
                let clean_syncs = response == "clean";
                let b = b.clone();
                let toast = toast.clone();
                let id = acc_id.clone();
                let name = acc_name.clone();
                let cb = cb_shared.clone();

                glib::MainContext::default().spawn_local(async move {
                    match b.sign_out_account(&id, clean_syncs).await {
                        Ok(()) => {
                            let msg = if clean_syncs {
                                format!("Signed out from “{name}” and removed local syncs")
                            } else {
                                format!("Signed out from “{name}” (local files preserved)")
                            };
                            toast.add_toast(adw::Toast::new(&msg));
                            cb();
                        }
                        Err(e) => {
                            toast.add_toast(adw::Toast::new(&format!("Failed to sign out: {e}")));
                        }
                    }
                });
            }
            d.close();
        });

        dialog.present();
    }
}
