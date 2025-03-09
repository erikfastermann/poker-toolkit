use crate::{cards::Cards, range::FullRangeTable};

pub unsafe fn init() {
    Cards::init();
    FullRangeTable::init();
}
