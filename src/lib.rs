/// One of the Fair Folk, the People of the Mounds, the sídhe, the aos sí, or any other fine name --- be very careful.
pub struct Faerie<'a> {
    _magic: &'a [u8]
}

impl<'a> Faerie<'a> {

    /// Be careful what you wish for
    pub fn summon (magic: &[u8]) -> Faerie {
        Faerie { _magic: magic }
    }

    pub fn is_magical(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {

    use super::Faerie;

    #[test]
    fn is_magical() {
        let magic = [0; 16];
        let faerie = Faerie::summon(&magic);
        assert!(faerie.is_magical());
    }
}
