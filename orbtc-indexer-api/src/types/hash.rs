use bitcoin::hashes::sha256d::Hash as BHash;
use bitcoin::{BlockHash, Txid};

#[rustfmt::skip]
#[cfg(feature = "diesel")]
use diesel::{
    pg::Pg,
    backend::Backend,
    deserialize::{self, FromSqlRow, FromSql},
    expression::AsExpression,
    serialize::{self, Output, ToSql},
    sql_types::Bytea,
};

#[derive(Default, Debug, Clone, Eq, PartialEq, PartialOrd, Ord, std::hash::Hash)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[cfg_attr(feature = "diesel", derive(FromSqlRow, AsExpression))]
#[cfg_attr(feature = "diesel", diesel(sql_type = Bytea))]
pub struct Hash(Vec<u8>);

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

impl std::str::FromStr for Hash {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v = hex::decode(s)?;
        Ok(Self(v.to_vec()))
    }
}

impl std::convert::TryFrom<String> for Hash {
    type Error = hex::FromHexError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let v = hex::decode(value)?;
        Ok(Self(v.to_vec()))
    }
}

impl std::convert::TryFrom<&str> for Hash {
    type Error = hex::FromHexError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let v = hex::decode(value)?;
        Ok(Self(v.to_vec()))
    }
}

impl std::convert::TryFrom<&[u8]> for Hash {
    type Error = hex::FromHexError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(value.to_vec()))
    }
}

impl From<&Txid> for Hash {
    fn from(value: &Txid) -> Self {
        let mut arr: [u8; 32] = *value.as_ref();
        arr.reverse();
        Self(arr.to_vec())
    }
}

impl From<Txid> for Hash {
    fn from(value: Txid) -> Self {
        let mut arr: [u8; 32] = *value.as_ref();
        arr.reverse();
        Self(arr.to_vec())
    }
}

impl From<BlockHash> for Hash {
    fn from(value: BlockHash) -> Self {
        let mut arr: [u8; 32] = *value.as_ref();
        arr.reverse();
        Self(arr.to_vec())
    }
}

impl From<&Hash> for Txid {
    fn from(value: &Hash) -> Txid {
        let mut arr = value.0.clone();
        arr.reverse();
        let mut digest: [u8; 32] = [0; 32];
        for (i, v) in arr.iter().enumerate() {
            digest[i] = *v;
        }
        Txid::from_raw_hash(*BHash::from_bytes_ref(&digest))
    }
}

impl From<&Hash> for BlockHash {
    fn from(value: &Hash) -> BlockHash {
        let mut arr = value.0.clone();
        arr.reverse();
        let mut digest: [u8; 32] = [0; 32];
        for (i, v) in arr.iter().enumerate() {
            digest[i] = *v;
        }
        BlockHash::from_raw_hash(*BHash::from_bytes_ref(&digest))
    }
}

impl Hash {
    pub fn sha2(data: impl AsRef<[u8]>) -> Self {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize().to_vec();
        Self(hash)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn to_hex_string(&self) -> String {
        self.to_string()
    }
}

#[cfg(feature = "diesel")]
impl ToSql<Bytea, Pg> for Hash
where
    [u8]: ToSql<Bytea, Pg>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        <[u8] as ToSql<Bytea, Pg>>::to_sql(&self.0, out)
    }
}

#[cfg(feature = "diesel")]
impl FromSql<Bytea, diesel::pg::Pg> for Hash
where
    Vec<u8>: FromSql<Bytea, diesel::pg::Pg>,
{
    fn from_sql(bytes: <Pg as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        let val = <Vec<u8> as FromSql<Bytea, Pg>>::from_sql(bytes)?;
        match Hash::try_from(val.as_slice()) {
            Ok(v) => Ok(v),
            Err(err) => Err(Box::new(err)),
        }
    }
}

impl serde::Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Hash, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BigDecimalVisitor;

        impl serde::de::Visitor<'_> for BigDecimalVisitor {
            type Value = Hash;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a hex-encoded string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Hash, E>
            where
                E: serde::de::Error,
            {
                use std::str::FromStr;
                Hash::from_str(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(BigDecimalVisitor)
    }
}
