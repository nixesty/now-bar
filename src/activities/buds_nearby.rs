//! Buds2Popup's lid detection, moved into the Now Bar: a Galaxy Buds advert carries a counter
//! that goes up each time the case opens; a change while the Buds aren't connected to this PC
//! offers to connect them. Pure: `buds_nearby_worker.rs` listens, the app offers.
//!
//! The advert (Buds2, from Buds2Popup's captures): a local name with "Buds" in one advert, and in
//! another from the same address Samsung's manufacturer data (company 0x0075) starting
//! `42 09 81 02 14 15`, the lid counter at [10]. Worn buds advertise too, every few seconds, with
//! the same counter.

use std::collections::HashSet;

/// Farther than this is someone else's, or behind a wall (Buds2Popup's threshold).
pub const MIN_RSSI: i16 = -72;
/// After the lid opens the Buds may connect by themselves: the offer waits this long.
pub const SETTLE_MS: u64 = 4_000;
/// No new offer this soon after the last.
pub const COOLDOWN_MS: u64 = 8_000;
/// An unanswered offer goes after this long.
pub const OFFER_MS: u64 = 30_000;
/// A connection that doesn't come by then failed.
pub const CONNECT_MS: u64 = 20_000;
/// How long "Bağlandı" and "Bağlanamadı" stay.
pub const CONNECTED_MS: u64 = 4_000;
pub const FAILED_MS: u64 = 10_000;

const SAMSUNG: u16 = 0x0075;
const BUDS_ADVERT: [u8; 6] = [0x42, 0x09, 0x81, 0x02, 0x14, 0x15];
const LID_AT: usize = 10;

/// The lid counter a Galaxy Buds advert's manufacturer data carries.
pub fn lid_counter(company: u16, data: &[u8]) -> Option<u8> {
    (company == SAMSUNG && data.starts_with(&BUDS_ADVERT)).then(|| data.get(LID_AT).copied()).flatten()
}

pub fn is_buds_name(name: &str) -> bool {
    name.to_lowercase().contains("buds")
}

/// What the adverts said so far: the addresses that named themselves Buds, the last counter.
#[derive(Debug, Default)]
pub struct Lid {
    pub last: Option<u8>,
    buds: HashSet<u64>,
}

impl Lid {
    /// `last`: the counter remembered from before, so the first opening after a start counts.
    pub fn new(last: Option<u8>) -> Lid {
        Lid { last, buds: HashSet::new() }
    }

    /// An advert from `address`; the new counter when the lid opened (it changed).
    pub fn advert(&mut self, address: u64, name: &str, counter: Option<u8>, rssi: i16) -> Option<u8> {
        if is_buds_name(name) {
            self.buds.insert(address);
        }
        let counter = counter?;
        if !self.buds.contains(&address) || rssi < MIN_RSSI {
            return None;
        }
        let before = self.last.replace(counter);
        before.is_some_and(|b| b != counter).then_some(counter)
    }
}

/// The offer to connect, as it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    Asking { since: u64 },
    Connecting { since: u64 },
    Connected { at: u64 },
    Failed { at: u64 },
}

impl Offer {
    /// The offer at `now`: an unanswered one and a shown result go, a connection that doesn't
    /// come fails.
    pub fn at(self, now: u64) -> Option<Offer> {
        let age = |t: u64| now.saturating_sub(t);
        match self {
            Offer::Asking { since } if age(since) > OFFER_MS => None,
            Offer::Connecting { since } if age(since) > CONNECT_MS => Some(Offer::Failed { at: since + CONNECT_MS }),
            Offer::Connected { at } if age(at) > CONNECTED_MS => None,
            Offer::Failed { at } if age(at) > FAILED_MS => None,
            o => Some(o),
        }
    }

    pub fn status(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Offer::Asking { .. } => tr("Bağlansın mı?"),
            Offer::Connecting { .. } => tr("Bağlanıyor…"),
            Offer::Connected { .. } => tr("Bağlandı"),
            Offer::Failed { .. } => tr("Bağlanamadı"),
        }
    }
}

/// A lid opening offers to connect unless the Buds are connected, one is under way, or the last
/// offer was too recent.
pub fn should_offer(connected: bool, current: Option<Offer>, last_offer_ms: Option<u64>, now: u64) -> bool {
    let busy = matches!(current, Some(Offer::Asking { .. } | Offer::Connecting { .. } | Offer::Connected { .. }));
    !connected && !busy && last_offer_ms.is_none_or(|t| now.saturating_sub(t) >= COOLDOWN_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up Galaxy Buds advert's manufacturer data with the lid counter at [10].
    fn samsung(counter: u8) -> Vec<u8> {
        vec![0x42, 0x09, 0x81, 0x02, 0x14, 0x15, 0x03, 0x21, 0x01, 0x09, counter, 0x5A, 0x00]
    }

    #[test]
    fn the_lid_counter_from_samsungs_advert() {
        assert_eq!(lid_counter(0x0075, &samsung(0x83)), Some(0x83));
        assert_eq!(lid_counter(0x004C, &samsung(0x83)), None, "Apple's");
        assert_eq!(lid_counter(0x0075, &samsung(0x83)[..10]), None, "too short");
        let mut other = samsung(0x83);
        other[3] = 0x07;
        assert_eq!(lid_counter(0x0075, &other), None, "another Samsung advert");
    }

    #[test]
    fn buds_by_their_advertised_name() {
        assert!(is_buds_name("Buds2"));
        assert!(is_buds_name("Galaxy Buds2 Pro"));
        assert!(is_buds_name("BUDS FE"));
        assert!(!is_buds_name(""));
        assert!(!is_buds_name("JBL Tune 510BT"));
    }

    #[test]
    fn the_lid_opening_is_a_change_of_the_counter() {
        let mut lid = Lid::new(None);
        assert_eq!(lid.advert(0xA1, "Buds2", None, -50), None, "the name: now a known address");
        assert_eq!(lid.advert(0xA1, "", Some(0x82), -50), None, "first seen: nothing to compare");
        assert_eq!(lid.advert(0xA1, "", Some(0x82), -50), None, "worn: the same counter every few seconds");
        assert_eq!(lid.advert(0xA1, "", Some(0x83), -50), Some(0x83));
        assert_eq!(lid.last, Some(0x83));
    }

    #[test]
    fn a_remembered_counter_makes_the_first_opening_count() {
        let mut lid = Lid::new(Some(0x83));
        lid.advert(0xA1, "Buds2", None, -50);
        assert_eq!(lid.advert(0xA1, "", Some(0x84), -60), Some(0x84));
    }

    #[test]
    fn far_and_unknown_buds_are_left_alone() {
        let mut lid = Lid::new(Some(0x10));
        lid.advert(0xA1, "Buds2", None, -50);
        assert_eq!(lid.advert(0xA1, "", Some(0x11), MIN_RSSI - 1), None, "too far: someone else's, or behind a wall");
        assert_eq!(lid.last, Some(0x10), "and not counted");
        assert_eq!(lid.advert(0xB2, "", Some(0x11), -40), None, "an address that never said a Buds name");
        assert_eq!(lid.advert(0xA1, "", Some(0x11), MIN_RSSI), Some(0x11));
    }

    #[test]
    fn an_offer_over_time() {
        let asking = Offer::Asking { since: 1_000 };
        assert_eq!(asking.at(1_000 + OFFER_MS), Some(asking));
        assert_eq!(asking.at(1_000 + OFFER_MS + 1), None, "unanswered, it goes");
        let connecting = Offer::Connecting { since: 1_000 };
        assert_eq!(connecting.at(1_000 + CONNECT_MS + 1), Some(Offer::Failed { at: 1_000 + CONNECT_MS }), "they never came");
        let done = Offer::Connected { at: 1_000 };
        assert_eq!(done.at(1_000 + CONNECTED_MS + 1), None);
        let failed = Offer::Failed { at: 1_000 };
        assert_eq!(failed.at(1_000 + FAILED_MS), Some(failed));
        assert_eq!(failed.at(1_000 + FAILED_MS + 1), None);
    }

    #[test]
    fn the_offers_words() {
        assert_eq!(Offer::Asking { since: 0 }.status(), "Bağlansın mı?");
        assert_eq!(Offer::Connecting { since: 0 }.status(), "Bağlanıyor…");
        assert_eq!(Offer::Connected { at: 0 }.status(), "Bağlandı");
        assert_eq!(Offer::Failed { at: 0 }.status(), "Bağlanamadı");
    }

    #[test]
    fn when_a_lid_opening_becomes_an_offer() {
        assert!(should_offer(false, None, None, 10_000));
        assert!(!should_offer(true, None, None, 10_000), "they connected by themselves");
        assert!(!should_offer(false, Some(Offer::Connecting { since: 9_000 }), None, 10_000));
        assert!(!should_offer(false, None, Some(10_000 - COOLDOWN_MS + 1), 10_000), "one just went");
        assert!(should_offer(false, None, Some(10_000 - COOLDOWN_MS), 10_000));
        assert!(should_offer(false, Some(Offer::Failed { at: 1_000 }), None, 10_000), "a new opening after a failure");
    }
}
