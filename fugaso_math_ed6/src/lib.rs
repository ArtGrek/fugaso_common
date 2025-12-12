pub mod config;
pub mod math;
pub mod protocol;
pub mod rand;

#[cfg(test)]
mod tests {
    use crate::math::ThunderExpressMath;
    #[test]
    pub fn test_create() {
        let math = ThunderExpressMath::new(None, None);
        assert!(math.is_ok());
    }
}
