use std::sync::Once;

use crate::{cards::Cards, hand::Hand};

static INIT: Once = Once::new();

pub unsafe fn init() {
    INIT.call_once(|| {
        Cards::init();
        Hand::init();
    });
}
