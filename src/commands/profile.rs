//! `cpmfl profile` — your homeowner communication and directory-privacy
//! settings.
//!
//! Read-only, like the rest. Changing any of these is a portal capability this
//! CLI deliberately doesn't expose (`PATCH /Homeowner/{id}/Preferences`); see
//! `docs/api.md`.

use pk_cli_core::{output, CliError};
use serde_json::json;

use super::{emit, Ctx};
use crate::config::Service;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let homeowner = ctx.homeowner_id(&client)?;
    let prefs = client.get(
        Service::Users,
        &format!("/Homeowner/{homeowner}/Preferences"),
        &[],
    )?;

    let payload = json!({
        "email": prefs.get("email"),
        "communication": {
            "billing": prefs.get("billingCommunicationPreference"),
            "general": prefs.get("generalCommunicationPreference"),
            "pay_billing_text": prefs.get("receivePayBillingText"),
            "pay_confirmation_email": prefs.get("sendPayConfirmationEmail"),
        },
        // What the community directory shows other residents about you.
        "directory_privacy": {
            "hide_name": prefs.get("hideNameInDirectory"),
            "hide_email": prefs.get("hideEmailInDirectory"),
            "hide_phone": prefs.get("hidePhoneInDirectory"),
            "hide_property": prefs.get("hidePropertyInDirectory"),
        },
        "mailing_address_id": prefs.get("mailingAddressId"),
    });

    emit(ctx, "homeowner-profile", payload, |v| output::kv(v, 0));
    Ok(())
}
