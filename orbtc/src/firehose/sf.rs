pub mod firehose {
    pub mod v2 {
        tonic::include_proto!("sf.firehose.v2");
    }
}

pub mod bitcoin {
    pub mod v1 {
        tonic::include_proto!("sf.bitcoin.v1");
    }
}
