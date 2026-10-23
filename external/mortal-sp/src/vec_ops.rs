use std::ops::AddAssign;

/// Expecting some SIMD optimizations.
#[inline]
pub(crate) fn vec_add_assign<L, R>(lhs: &mut [L], rhs: &[R])
where
    L: Copy + AddAssign<R>,
    R: Copy,
{
    assert_eq!(
        lhs.len(),
        rhs.len(),
        "vec_add_assign length mismatch: lhs={}, rhs={}",
        lhs.len(),
        rhs.len(),
    );
    lhs.iter_mut().zip(rhs).for_each(|(l, &r)| *l += r);
}
