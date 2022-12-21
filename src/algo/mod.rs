mod generic;

pub use self::generic::*;

// OPT: Use a super optimizer to find optimal sequences for add, mul, neg, sub,
// and shift.

#[cfg(feature = "criterion")]
#[doc(hidden)]
pub mod bench {
    use super::*;
    use criterion::Criterion;

    pub fn group(criterion: &mut Criterion) {
        generic::bench::group(criterion);
    }
}
