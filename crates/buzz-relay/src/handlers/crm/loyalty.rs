//! Loyalty programme (kind 42170), manual bonus adjustments (kind 42171) and
//! the bonuses a sale earns and spends (kinds 42104, 42105).

use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_core::CommunityId;
use buzz_crm::{
    tags, BonusAdjustPayload, CrmAction, CrmError, LoyaltySettingsPayload, SalePayload,
};
use buzz_db::crm::loyalty::{self as store, BonusMoveInsert, BonusSource, LoyaltySettingsRecord};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

fn rejected(message: impl std::fmt::Display) -> IngestError {
    IngestError::Rejected(format!("invalid: {message}"))
}

fn bp(value: u32, field: &'static str) -> Result<i32, IngestError> {
    i32::try_from(value).map_err(|_| invalid(CrmError::field(field, "is out of range")))
}

fn payload_of(record: LoyaltySettingsRecord) -> LoyaltySettingsPayload {
    LoyaltySettingsPayload {
        v: 1,
        enabled: record.enabled,
        cashback_bp: u32::try_from(record.cashback_bp).unwrap_or(0),
        max_redeem_bp: u32::try_from(record.max_redeem_bp).unwrap_or(0),
    }
}

/// A kind 42170 event whose payload passed pure validation.
pub(super) struct PreparedLoyalty(LoyaltySettingsPayload);

impl PreparedLoyalty {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        LoyaltySettingsPayload::parse(&event.content)
            .map(Self)
            .map_err(invalid)
    }

    pub(super) async fn apply(
        self,
        tx: &mut Transaction<'static, Postgres>,
        tenant: &TenantContext,
        event: &Event,
        channel_id: Uuid,
    ) -> Result<(), IngestError> {
        let actor = event.pubkey.to_bytes();
        // Exclusive: a sale must not read the programme halfway through a change.
        require_live_space(tx, tenant, channel_id, &actor, CrmAction::ManageSpace, true).await?;
        let settings = LoyaltySettingsRecord {
            enabled: self.0.enabled,
            cashback_bp: bp(self.0.cashback_bp, "cashback_bp")?,
            max_redeem_bp: bp(self.0.max_redeem_bp, "max_redeem_bp")?,
        };
        store::write_loyalty_settings(
            tx,
            tenant.community(),
            channel_id,
            settings,
            event.id.as_bytes(),
        )
        .await
        .map_err(|e| db_error("write CRM loyalty settings", e))
    }
}

/// A kind 42171 event whose payload and tags passed pure validation.
pub(super) struct PreparedBonusAdjust {
    adjustment_id: Uuid,
    payload: BonusAdjustPayload,
}

impl PreparedBonusAdjust {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        Ok(Self {
            adjustment_id: tags::crm_id(tag_slices(event)).map_err(invalid)?,
            payload: BonusAdjustPayload::parse(&event.content).map_err(invalid)?,
        })
    }

    pub(super) async fn apply(
        self,
        tx: &mut Transaction<'static, Postgres>,
        tenant: &TenantContext,
        event: &Event,
        channel_id: Uuid,
    ) -> Result<(), IngestError> {
        let actor = event.pubkey.to_bytes();
        // Exclusive: balances change only under the space lock (see store).
        require_live_space(tx, tenant, channel_id, &actor, CrmAction::ManageSpace, true).await?;
        let community = tenant.community();
        let customer_id = self.payload.customer_id;
        let balance = store::balance_for_write(tx, community, channel_id, customer_id)
            .await
            .map_err(|e| db_error("read CRM bonus balance", e))?
            .ok_or_else(|| rejected("customer_id: not a party of this CRM space"))?;
        if self.payload.delta_minor < 0 && balance + self.payload.delta_minor < 0 {
            return Err(rejected(format!(
                "delta_minor: the customer has {balance} kopecks of bonuses"
            )));
        }
        let inserted = store::insert_bonus_move(
            tx,
            community,
            BonusMoveInsert {
                space_channel_id: channel_id,
                customer_id,
                source: BonusSource::Manual,
                delta_minor: self.payload.delta_minor,
                sale_id: None,
                adjustment_id: Some(self.adjustment_id),
                reason: Some(&self.payload.reason),
                event_id: event.id.as_bytes(),
                actor: &actor,
            },
        )
        .await
        .map_err(|e| db_error("insert CRM bonus move", e))?;
        if inserted {
            Ok(())
        } else {
            Err(conflict("the bonus adjustment is already recorded"))
        }
    }
}

/// Spend and earn the bonuses of a sale being posted, inside its transaction
/// (the caller holds the exclusive space lock).
pub(super) async fn apply_sale_bonuses(
    tx: &mut Transaction<'static, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    sale_id: Uuid,
    payload: &SalePayload,
    event: &Event,
) -> Result<(), IngestError> {
    let Some(customer_id) = payload.customer_id else {
        // Pure validation already refuses bonuses without a customer.
        return Ok(());
    };
    let settings = store::loyalty_settings_for_write(tx, community, channel_id)
        .await
        .map_err(|e| db_error("read CRM loyalty settings", e))?
        .map(payload_of);
    let total = payload.total().map_err(invalid)?.minor();
    let actor = event.pubkey.to_bytes();
    let mv = |source, delta_minor| BonusMoveInsert {
        space_channel_id: channel_id,
        customer_id,
        source,
        delta_minor,
        sale_id: Some(sale_id),
        adjustment_id: None,
        reason: None,
        event_id: event.id.as_bytes(),
        actor: &actor,
    };

    if let Some(bonus) = payload.bonus_minor.map(|bonus| bonus.minor()) {
        let settings = settings
            .filter(|settings| settings.enabled)
            .ok_or_else(|| rejected("bonus_minor: the loyalty programme is off"))?;
        let balance = store::balance_for_write(tx, community, channel_id, customer_id)
            .await
            .map_err(|e| db_error("read CRM bonus balance", e))?
            .unwrap_or(0);
        if bonus > balance {
            return Err(rejected(format!(
                "bonus_minor: the customer has {balance} kopecks of bonuses"
            )));
        }
        let limit = settings.max_redeem_minor(total.saturating_add(bonus));
        if bonus > limit {
            return Err(rejected(format!(
                "bonus_minor: at most {limit} kopecks of this sale may be paid with bonuses"
            )));
        }
        store::insert_bonus_move(tx, community, mv(BonusSource::Redeem, -bonus))
            .await
            .map_err(|e| db_error("insert CRM bonus move", e))?;
    }

    let earned = settings.map_or(0, |settings| settings.cashback_minor(total));
    if earned > 0 {
        store::insert_bonus_move(tx, community, mv(BonusSource::Cashback, earned))
            .await
            .map_err(|e| db_error("insert CRM bonus move", e))?;
    }
    Ok(())
}
