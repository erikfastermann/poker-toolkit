use std::sync::Once;

use crate::{cards::Cards, range::RangeTable};

static INIT: Once = Once::new();

pub unsafe fn init() {
    INIT.call_once(|| {
        Cards::init();
        RangeTable::init();
    });
}
