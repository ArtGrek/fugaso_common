pub mod config;
pub mod math;
pub mod protocol;
pub mod rand;

#[cfg(test)]
mod tests {
    use crate::math::MegaThunderMath;
    #[test]
    pub fn test_create() {
        let math = MegaThunderMath::new(None, None);
        assert!(math.is_ok());
    }
}
