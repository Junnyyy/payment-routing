/// SplitMix64, specified here so replays never depend on a dependency's RNG
/// defaults. Wrapping is intentional ONLY in this pseudo-random state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Random(pub u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    pub fn inclusive(&mut self, min: u64, max: u64) -> u64 {
        let span = max - min;
        if span == u64::MAX {
            return self.next();
        }
        let bound = span + 1;
        // Reject the incomplete residue block to avoid modulo bias.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next();
            if value >= threshold {
                return min + value % bound;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Random;

    #[test]
    fn splitmix64_seed_zero_matches_fixed_vectors() {
        let mut random = Random(0);
        assert_eq!(random.next(), 0xe220a8397b1dcdaf);
        assert_eq!(random.next(), 0x6e789e6aa1b965f4);
        assert_eq!(random.next(), 0x06c45d188009454f);
        assert_eq!(Random(0).inclusive(0, u64::MAX), 0xe220a8397b1dcdaf);
    }
}
