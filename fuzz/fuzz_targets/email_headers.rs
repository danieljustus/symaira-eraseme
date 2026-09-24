#![no_main]

use libfuzzer_sys::fuzz_target;
use symeraseme_core::email::{FetchedMessage, parse_fetched_message};

fuzz_target!(|input: &[u8]| {
    let message = FetchedMessage {
        uid: 1,
        header: input.to_vec(),
        body: Vec::new(),
        flags: None,
        internal_date: None,
    };
    let _ = parse_fetched_message(&message);
});
