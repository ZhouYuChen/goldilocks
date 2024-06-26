//! Gustavson FG, Walker DW. Algorithms for in-place matrix transposition.
//! Concurrency and Computation Practice Experience 2019
//! <https://doi.org/10.1002/cpe.5071>

use std::{cmp::min, sync::Arc};

use super::{copy, square, Permute};

pub struct Gw18<T: 'static + Copy + Send + Sync> {
    rows:                usize,
    cols:                usize,
    square:              usize,
    transpose_square:    Arc<dyn Permute<T>>,
    transpose_remainder: Arc<dyn Permute<T>>,
    parallel:            bool,
}

impl<T: 'static + Copy + Send + Sync> Gw18<T> {
    pub fn new((rows, cols): (usize, usize)) -> Self {
        assert!(rows > 1 && cols > 1);
        let size = rows * cols;

        let (square, remainder) = if rows > cols {
            (cols, (rows % cols, cols))
        } else {
            (rows, (rows, cols % rows))
        };
        let transpose_square = super::transpose_strategy((square, square));
        let transpose_remainder = super::transpose_strategy(remainder);

        Self {
            rows,
            cols,
            square,
            transpose_square,
            transpose_remainder,
            parallel: size > 1 << 17,
        }
    }

    fn transpose_join(&self, values: &mut [T], blocks: usize) {
        let size = self.square;

        if blocks == 1 {
            self.transpose_square.permute(values);
        } else {
            // Recurse by splitting into two halves
            let blocks_top = blocks / 2;
            let blocks_bottom = blocks - blocks_top;
            let (top, bottom) = values.split_at_mut(blocks_top * size * size);

            if self.parallel {
                transpose_join(top, blocks_top, size);
                transpose_join(bottom, blocks_bottom, size);
            } else {
                rayon::join(
                    || transpose_join(top, blocks_top, size),
                    || transpose_join(bottom, blocks_bottom, size),
                );
            }

            // Merge the two halves
            shuffle(values, blocks_top * size, blocks_bottom * size, size);
        }
    }
}

impl<T: 'static + Copy + Send + Sync> Permute<T> for Gw18<T> {
    fn len(&self) -> usize {
        self.rows * self.cols
    }

    fn permute(&self, values: &mut [T]) {
        let (rows, cols) = (self.rows, self.cols);
        assert_eq!(values.len(), rows * cols);

        if self.rows > self.cols {
            // Divide into (cols × cols) squares and remainder
            let (squares, remainder) = (rows / cols, rows % cols);
            if remainder == 0 {
                transpose_join(values, squares, cols);
            } else {
                let (head, tail) = values.split_at_mut(squares * cols * cols);

                if !self.parallel {
                    // Transpose and join squares
                    transpose_join(head, squares, cols);

                    // Transpose remainder
                    self.transpose_remainder.permute(tail);
                } else {
                    rayon::join(
                        || transpose_join(head, squares, cols),
                        || self.transpose_remainder.permute(tail),
                    );
                }

                // Merge remainder into big matrix.
                shuffle(values, squares * cols, remainder, cols);
            }
        } else {
            // Divide into (rows × rows) squares and remainder
            let (squares, remainder) = (cols / rows, cols % rows);
            if remainder == 0 {
                partition_transpose(values, squares, rows);
            } else {
                // Split remainder from matrix.
                unshuffle(values, squares * rows, remainder, rows);
                let (head, tail) = values.split_at_mut(squares * rows * rows);

                if !self.parallel {
                    // Partition and transpose the squares
                    partition_transpose(head, squares, rows);

                    // Transpose remainder
                    self.transpose_remainder.permute(tail);
                } else {
                    rayon::join(
                        || partition_transpose(head, squares, rows),
                        || self.transpose_remainder.permute(tail),
                    );
                }
            }
        }
    }
}

pub fn transpose<T: Copy + Send + Sync>(values: &mut [T], (rows, cols): (usize, usize)) {
    const COPY_THRESHOLD: usize = if !cfg!(test) { 1 << 14 } else { 4 };
    const PAR_THRESHOLD: usize = 1 << 17;

    // eprintln!("transpose({rows}, {cols})");
    assert_eq!(values.len(), rows * cols);
    let size = rows * cols;

    if rows <= 1 || cols <= 1 {
        return;
    } else if rows == cols {
        square::transpose(values, rows)
    } else if size <= COPY_THRESHOLD {
        copy::transpose(values, (rows, cols));
    } else if rows > cols {
        // Divide into (cols × cols) squares and remainder
        let (squares, remainder) = (rows / cols, rows % cols);
        if remainder == 0 {
            transpose_join(values, squares, cols);
        } else {
            let (head, tail) = values.split_at_mut(squares * cols * cols);

            if size < PAR_THRESHOLD {
                // Transpose and join squares
                transpose_join(head, squares, cols);

                // Transpose remainder
                transpose(tail, (remainder, cols));
            } else {
                rayon::join(
                    || transpose_join(head, squares, cols),
                    || transpose(tail, (remainder, cols)),
                );
            }

            // Merge remainder into big matrix.
            shuffle(values, squares * cols, remainder, cols);
        }
    } else {
        // Divide into (rows × rows) squares and remainder
        let (squares, remainder) = (cols / rows, cols % rows);
        if remainder == 0 {
            partition_transpose(values, squares, rows);
        } else {
            // Split remainder from matrix.
            unshuffle(values, squares * rows, remainder, rows);
            let (head, tail) = values.split_at_mut(squares * rows * rows);

            if size < PAR_THRESHOLD {
                // Partition and transpose the squares
                partition_transpose(head, squares, rows);

                // Transpose remainder
                transpose(tail, (rows, remainder));
            } else {
                rayon::join(
                    || partition_transpose(head, squares, rows),
                    || transpose(tail, (rows, remainder)),
                );
            }
        }
    }
}

fn transpose_join<T: Copy + Send>(values: &mut [T], blocks: usize, size: usize) {
    const PAR_THRESHOLD: usize = 1 << 17;
    // eprintln!("transpose_join({blocks}, {size})");
    let n = blocks * size * size;
    debug_assert_eq!(values.len(), n);
    debug_assert!(!values.is_empty());

    if blocks == 1 {
        square::transpose(values, size);
    } else {
        // Recurse by splitting into two halves
        let blocks_top = blocks / 2;
        let blocks_bottom = blocks - blocks_top;
        let (top, bottom) = values.split_at_mut(blocks_top * size * size);

        if n < PAR_THRESHOLD {
            transpose_join(top, blocks_top, size);
            transpose_join(bottom, blocks_bottom, size);
        } else {
            rayon::join(
                || transpose_join(top, blocks_top, size),
                || transpose_join(bottom, blocks_bottom, size),
            );
        }

        // Merge the two halves
        shuffle(values, blocks_top * size, blocks_bottom * size, size);
    }
}

fn partition_transpose<T: Copy + Send>(values: &mut [T], blocks: usize, size: usize) {
    const PAR_THRESHOLD: usize = 1 << 17;
    // eprintln!("partition({blocks}, {size})");
    let n = blocks * size * size;
    debug_assert_eq!(values.len(), n);
    debug_assert!(!values.is_empty());

    if blocks == 1 {
        square::transpose(values, size);
    } else {
        // Recurse by splitting into two halves
        let blocks_top = blocks / 2;
        let blocks_bottom = blocks - blocks_top;

        // Split the two halves
        unshuffle(values, blocks_top * size, blocks_bottom * size, size);
        let (top, bottom) = values.split_at_mut(blocks_top * size * size);

        if n < PAR_THRESHOLD {
            partition_transpose(top, blocks_top, size);
            partition_transpose(bottom, blocks_bottom, size);
        } else {
            rayon::join(
                || partition_transpose(top, blocks_top, size),
                || partition_transpose(bottom, blocks_bottom, size),
            );
        }
    }
}

///
fn shuffle<T: Copy + Send>(values: &mut [T], a: usize, b: usize, m: usize) {
    const PAR_THRESHOLD: usize = 1 << 20;
    // eprintln!("shuffle({la}, {lb}, {m})");
    debug_assert_eq!(values.len(), (a + b) * m);
    let size = values.len();

    // Base case
    if m <= 1 {
        return;
    }

    // Split into left and right half
    let m_left = largest_power_of_two(m);
    let m_right = m - m_left;
    if (a * m_right > 0) & (b * m_left > 0) {
        let start = a * m_left;
        let end = start + a * m_right + b * m_left;
        exchange(&mut values[start..end], a * m_right, b * m_left);
    }
    let (left, right) = values.split_at_mut((a + b) * m_left);

    // Recurse
    if size < PAR_THRESHOLD {
        shuffle(left, a, b, m_left);
        shuffle(right, a, b, m_right);
    } else {
        rayon::join(
            || shuffle(left, a, b, m_left),
            || shuffle(right, a, b, m_right),
        );
    }
}

/// Given a vector of length `(a + b) * m` in pattern (a ‖ b) * m unshuffle it
/// into a a pattern (a * m ‖ b * m).
fn unshuffle<T: Copy + Send>(values: &mut [T], a: usize, b: usize, m: usize) {
    const PAR_THRESHOLD: usize = 1 << 20;
    // eprintln!("unshuffle({a}, {b}, {m})");
    debug_assert_eq!(values.len(), (a + b) * m);
    let size = values.len();

    // Base case
    if m <= 1 {
        return;
    }

    // Split the problem in two halves
    // Largest power of two less than `m`
    let m_left = largest_power_of_two(m);
    let m_right = m - m_left;
    let (left, right) = values.split_at_mut((a + b) * m_left);

    if size < PAR_THRESHOLD {
        unshuffle(left, a, b, m_left);
        unshuffle(right, a, b, m_right);
    } else {
        rayon::join(
            || unshuffle(left, a, b, m_left),
            || unshuffle(right, a, b, m_right),
        );
    }

    if (a * m_right > 0) & (b * m_left > 0) {
        let start = a * m_left;
        let end = start + b * m_left + a * m_right;
        exchange(&mut values[start..end], b * m_left, a * m_right);
    }
}

/// Given a pattern (a ‖ b) turn it into (b ‖ a).
fn exchange<T: Copy>(values: &mut [T], a: usize, b: usize) {
    // eprintln!("exchange({a}, {b})");
    debug_assert_eq!(values.len(), a + b);
    if a == b {
        let (left, right) = values.split_at_mut(a);
        left.swap_with_slice(right);
    } else if a > b {
        let (left, remainder) = values.split_at_mut(b);
        let (_middle, right) = remainder.split_at_mut(a - b);
        left.swap_with_slice(right);
        exchange(&mut values[b..], a - b, b);
    } else {
        let (left, remainder) = values.split_at_mut(a);
        let (_middle, right) = remainder.split_at_mut(b - a);
        left.swap_with_slice(right);
        exchange(&mut values[..b], a, b - a);
    }
}

fn largest_power_of_two(m: usize) -> usize {
    2_usize.pow((m as f64 - 1.).log2() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::divisors::{divisors, split};

    #[test]
    fn test_transpose_32_16() {
        test_size(32, 16);
    }

    #[test]
    fn test_transpose_16_32() {
        test_size(16, 32);
    }

    #[test]
    fn test_transpose_17_20() {
        test_size(17, 20);
    }

    #[test]
    fn test_transpose() {
        for size in divisors()
            .iter()
            .filter(|n| **n <= (1 << 16))
            .map(|n| *n as usize)
        {
            let a = split(size);
            let b = size / a;
            test_size(a, b);
            test_size(b, a);
        }
    }

    fn test_size(rows: usize, cols: usize) {
        let size = rows * cols;
        let mut matrix = (0_u64..size as u64).collect::<Vec<_>>();
        let mut reference = matrix.clone();
        transpose(&mut matrix, (rows, cols));
        copy::transpose(&mut reference, (rows, cols));
        assert_eq!(matrix, reference);
    }
}
