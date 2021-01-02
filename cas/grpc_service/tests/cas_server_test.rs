// Copyright 2020 Nathan (Blaise) Bruer.  All rights reserved.

use tokio::io::Error;
use tonic::Request;

use proto::build::bazel::remote::execution::v2::Digest;
use proto::google::rpc::Status as GrpcStatus;

use cas_server::CasServer;
use proto::build::bazel::remote::execution::v2::content_addressable_storage_server::ContentAddressableStorage;
use store::{create_store, StoreType};

const INSTANCE_NAME: &str = "foo";
const HASH1: &str = "0123456789abcdef000000000000000000000000000000000123456789abcdef";
const HASH2: &str = "9993456789abcdef000000000000000000000000000000000123456789abc999";
const HASH3: &str = "7773456789abcdef000000000000000000000000000000000123456789abc777";
const BAD_HASH: &str = "BAD_HASH";

#[cfg(test)]
mod find_missing_blobs {
    use super::*;
    use pretty_assertions::assert_eq; // Must be declared in every module.

    use std::io::Cursor;

    use proto::build::bazel::remote::execution::v2::FindMissingBlobsRequest;

    #[tokio::test]
    async fn empty_store() {
        let store = create_store(&StoreType::Memory);
        let cas_server = CasServer::new(store.clone());

        let raw_response = cas_server
            .find_missing_blobs(Request::new(FindMissingBlobsRequest {
                instance_name: INSTANCE_NAME.to_string(),
                blob_digests: vec![Digest {
                    hash: HASH1.to_string(),
                    size_bytes: 0,
                }],
            }))
            .await;
        assert!(raw_response.is_ok());
        let response = raw_response.unwrap().into_inner();
        assert_eq!(response.missing_blob_digests.len(), 1);
    }

    #[tokio::test]
    async fn store_one_item_existence() -> Result<(), Error> {
        let store = create_store(&StoreType::Memory);
        let cas_server = CasServer::new(store.clone());

        const VALUE: &str = "1";

        store
            .update(&HASH1, VALUE.len(), Box::new(Cursor::new(VALUE)))
            .await?;
        let raw_response = cas_server
            .find_missing_blobs(Request::new(FindMissingBlobsRequest {
                instance_name: INSTANCE_NAME.to_string(),
                blob_digests: vec![Digest {
                    hash: HASH1.to_string(),
                    size_bytes: VALUE.len() as i64,
                }],
            }))
            .await;
        assert!(raw_response.is_ok());
        let response = raw_response.unwrap().into_inner();
        assert_eq!(response.missing_blob_digests.len(), 0); // All items should have been found.
        Ok(())
    }

    #[tokio::test]
    async fn has_three_requests_one_bad_hash() -> Result<(), Error> {
        let store = create_store(&StoreType::Memory);
        let cas_server = CasServer::new(store.clone());

        const VALUE: &str = "1";

        store
            .update(&HASH1, VALUE.len(), Box::new(Cursor::new(VALUE)))
            .await?;
        let raw_response = cas_server
            .find_missing_blobs(Request::new(FindMissingBlobsRequest {
                instance_name: INSTANCE_NAME.to_string(),
                blob_digests: vec![
                    Digest {
                        hash: HASH1.to_string(),
                        size_bytes: VALUE.len() as i64,
                    },
                    Digest {
                        hash: BAD_HASH.to_string(),
                        size_bytes: VALUE.len() as i64,
                    },
                    Digest {
                        hash: HASH1.to_string(),
                        size_bytes: VALUE.len() as i64,
                    },
                ],
            }))
            .await;
        assert!(raw_response.is_ok());
        let response = raw_response.unwrap().into_inner();
        assert_eq!(
            response.missing_blob_digests,
            vec![Digest {
                hash: BAD_HASH.to_string(),
                size_bytes: VALUE.len() as i64,
            }]
        ); // All items should have been found.
        Ok(())
    }
}

#[cfg(test)]
mod batch_update_blobs {
    use super::*;
    use pretty_assertions::assert_eq; // Must be declared in every module.

    use std::io::Cursor;

    use proto::build::bazel::remote::execution::v2::{
        batch_update_blobs_request, batch_update_blobs_response, BatchUpdateBlobsRequest,
        BatchUpdateBlobsResponse,
    };

    #[tokio::test]
    async fn update_existing_item() {
        let store = create_store(&StoreType::Memory);
        let cas_server = CasServer::new(store.clone());

        const VALUE1: &str = "1";
        const VALUE2: &str = "23";

        let digest = Digest {
            hash: HASH1.to_string(),
            size_bytes: VALUE2.len() as i64,
        };

        store
            .update(&HASH1, VALUE1.len(), Box::new(Cursor::new(VALUE1)))
            .await
            .expect("Update should have succeeded");

        let raw_response = cas_server
            .batch_update_blobs(Request::new(BatchUpdateBlobsRequest {
                instance_name: INSTANCE_NAME.to_string(),
                requests: vec![batch_update_blobs_request::Request {
                    digest: Some(digest.clone()),
                    data: VALUE2.into(),
                }],
            }))
            .await;
        assert!(raw_response.is_ok());
        assert_eq!(
            raw_response.unwrap().into_inner(),
            BatchUpdateBlobsResponse {
                responses: vec![batch_update_blobs_response::Response {
                    digest: Some(digest),
                    status: Some(GrpcStatus {
                        code: 0, // Status Ok.
                        message: "".to_string(),
                        details: vec![],
                    }),
                },],
            }
        );
        let mut new_data = Vec::new();
        store
            .get(&HASH1, HASH1.len(), &mut Cursor::new(&mut new_data))
            .await
            .expect("Get should have succeeded");
        assert_eq!(
            new_data,
            VALUE2.as_bytes(),
            "Expected store to have been updated to new value"
        );
    }
}

#[cfg(test)]
mod batch_read_blobs {
    use super::*;
    use pretty_assertions::assert_eq; // Must be declared in every module.

    use std::io::Cursor;

    use proto::build::bazel::remote::execution::v2::{
        batch_read_blobs_response, BatchReadBlobsRequest, BatchReadBlobsResponse,
    };
    use tonic::Code;

    #[tokio::test]
    async fn batch_read_blobs_read_two_blobs_success_one_fail() -> Result<(), Error> {
        let store = create_store(&StoreType::Memory);
        let cas_server = CasServer::new(store.clone());

        const VALUE1: &str = "1";
        const VALUE2: &str = "23";

        let digest1 = Digest {
            hash: HASH1.to_string(),
            size_bytes: VALUE2.len() as i64,
        };
        let digest2 = Digest {
            hash: HASH2.to_string(),
            size_bytes: VALUE2.len() as i64,
        };
        {
            // Insert dummy data.
            store
                .update(&HASH1, VALUE1.len(), Box::new(Cursor::new(VALUE1)))
                .await
                .expect("Update should have succeeded");
            store
                .update(&HASH2, VALUE2.len(), Box::new(Cursor::new(VALUE2)))
                .await
                .expect("Update should have succeeded");
        }
        {
            // Read two blobs and additional blob should come back not found.
            let digest3 = Digest {
                hash: HASH3.to_string(),
                size_bytes: 3,
            };
            let raw_response = cas_server
                .batch_read_blobs(Request::new(BatchReadBlobsRequest {
                    instance_name: INSTANCE_NAME.to_string(),
                    digests: vec![digest1.clone(), digest2.clone(), digest3.clone()],
                }))
                .await;
            assert!(raw_response.is_ok());
            assert_eq!(
                raw_response.unwrap().into_inner(),
                BatchReadBlobsResponse {
                    responses: vec![
                        batch_read_blobs_response::Response {
                            digest: Some(digest1),
                            data: VALUE1.as_bytes().to_vec(),
                            status: Some(GrpcStatus {
                                code: 0, // Status Ok.
                                message: "".to_string(),
                                details: vec![],
                            }),
                        },
                        batch_read_blobs_response::Response {
                            digest: Some(digest2),
                            data: VALUE2.as_bytes().to_vec(),
                            status: Some(GrpcStatus {
                                code: 0, // Status Ok.
                                message: "".to_string(),
                                details: vec![],
                            }),
                        },
                        batch_read_blobs_response::Response {
                            digest: Some(digest3.clone()),
                            data: vec![],
                            status: Some(GrpcStatus {
                                code: Code::NotFound as i32,
                                message: format!("Error: Custom {{ kind: NotFound, error: \"Trying to get object but could not find hash: {}\" }}", digest3.hash),
                                details: vec![],
                            }),
                        }
                    ],
                }
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod end_to_end {
    use super::*;
    use pretty_assertions::assert_eq; // Must be declared in every module.

    use proto::build::bazel::remote::execution::v2::{
        batch_update_blobs_request, batch_update_blobs_response, BatchUpdateBlobsRequest,
        BatchUpdateBlobsResponse, FindMissingBlobsRequest,
    };

    #[tokio::test]
    async fn batch_update_blobs_two_items_existence_with_third_missing() -> Result<(), Error> {
        let store = create_store(&StoreType::Memory);
        let cas_server = CasServer::new(store.clone());

        const VALUE1: &str = "1";
        const VALUE2: &str = "23";

        let digest1 = Digest {
            hash: HASH1.to_string(),
            size_bytes: VALUE1.len() as i64,
        };
        let digest2 = Digest {
            hash: HASH2.to_string(),
            size_bytes: VALUE2.len() as i64,
        };

        {
            // Send update to insert two entries into backend.
            let raw_response = cas_server
                .batch_update_blobs(Request::new(BatchUpdateBlobsRequest {
                    instance_name: INSTANCE_NAME.to_string(),
                    requests: vec![
                        batch_update_blobs_request::Request {
                            digest: Some(digest1.clone()),
                            data: VALUE1.into(),
                        },
                        batch_update_blobs_request::Request {
                            digest: Some(digest2.clone()),
                            data: VALUE2.into(),
                        },
                    ],
                }))
                .await;
            assert!(raw_response.is_ok());
            assert_eq!(
                raw_response.unwrap().into_inner(),
                BatchUpdateBlobsResponse {
                    responses: vec![
                        batch_update_blobs_response::Response {
                            digest: Some(digest1),
                            status: Some(GrpcStatus {
                                code: 0, // Status Ok.
                                message: "".to_string(),
                                details: vec![],
                            }),
                        },
                        batch_update_blobs_response::Response {
                            digest: Some(digest2),
                            status: Some(GrpcStatus {
                                code: 0, // Status Ok.
                                message: "".to_string(),
                                details: vec![],
                            }),
                        }
                    ],
                }
            );
        }
        {
            // Query the backend for inserted entries plus one that is not
            // present and ensure it only returns the one that is missing.
            let missing_digest = Digest {
                hash: HASH3.to_string(),
                size_bytes: 1,
            };
            let raw_response = cas_server
                .find_missing_blobs(Request::new(FindMissingBlobsRequest {
                    instance_name: INSTANCE_NAME.to_string(),
                    blob_digests: vec![
                        Digest {
                            hash: HASH1.to_string(),
                            size_bytes: VALUE1.len() as i64,
                        },
                        missing_digest.clone(),
                        Digest {
                            hash: HASH2.to_string(),
                            size_bytes: VALUE2.len() as i64,
                        },
                    ],
                }))
                .await;
            assert!(raw_response.is_ok());
            let response = raw_response.unwrap().into_inner();
            assert_eq!(response.missing_blob_digests, vec![missing_digest]);
        }
        Ok(())
    }
}