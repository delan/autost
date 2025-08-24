use bincode::de::BorrowDecoder;
use bincode::de::Decoder;
use bincode::enc::Encoder;
use bincode::error::DecodeError;
use bincode::BorrowDecode;
use bincode::Decode;
use bincode::Encode;

use std::fmt::Display;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Hash(pub blake3::Hash);

impl PartialOrd for Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            let hash = self.0.to_hex();
            write!(f, "{}...", &hash.as_str()[0..13])
        } else {
            write!(f, "{}", self.0.to_hex().as_str())
        }
    }
}

impl<__Context> Decode<__Context> for Hash {
    fn decode<D: Decoder<Context = __Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        Ok(Self(blake3::Hash::from_bytes(Decode::decode(decoder)?)))
    }
}

impl<'__de, __Context> BorrowDecode<'__de, __Context> for Hash {
    fn borrow_decode<D: BorrowDecoder<'__de, Context = __Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        Ok(Self(
            blake3::Hash::from_slice(BorrowDecode::borrow_decode(decoder)?)
                .map_err(|e| DecodeError::OtherString(e.to_string()))?,
        ))
    }
}

impl Encode for Hash {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        Encode::encode(self.0.as_bytes(), encoder)
    }
}
