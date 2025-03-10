use crate::{cards::Cards, range::RangeTable};

pub unsafe fn init() {
    Cards::init();
    RangeTable::init();
}
