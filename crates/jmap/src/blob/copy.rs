/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::download::BlobDownload;
use common::{
    Server,
    auth::AccessToken,
    za::{ZA_TRACE_MAIL_COPY, mail::za_keyless_refusal},
};
use email::message::za::view::ZaMailView;
use jmap_proto::{
    error::set::{SetError, SetErrorType},
    method::copy::{CopyBlobRequest, CopyBlobResponse},
    request::MaybeInvalid,
};
use registry::schema::enums::Permission;
use std::future::Future;
use store::write::{BatchBuilder, BlobLink, BlobOp, now};
use trc::AddContext;
use types::blob::{BlobClass, BlobId};
use utils::map::vec_map::VecMap;

pub trait BlobCopy: Sync + Send {
    fn blob_copy(
        &self,
        request: CopyBlobRequest,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<CopyBlobResponse>> + Send;
}

impl BlobCopy for Server {
    async fn blob_copy(
        &self,
        request: CopyBlobRequest,
        access_token: &AccessToken,
    ) -> trc::Result<CopyBlobResponse> {
        let mut response = CopyBlobResponse {
            from_account_id: request.from_account_id,
            account_id: request.account_id,
            copied: VecMap::with_capacity(request.blob_ids.len()),
            not_copied: VecMap::new(),
        };
        let account_id = request.account_id.document_id();

        // Mail spec 6.3, 9: no blob copy into or out of a key account.
        for za_account_id in [account_id, request.from_account_id.document_id()] {
            if self.za_is_key_account(za_account_id).await? {
                trc::event!(
                    Security(trc::SecurityEvent::Unauthorized),
                    AccountId = za_account_id,
                    Details = ZA_TRACE_MAIL_COPY,
                );
                return Err(za_keyless_refusal().account_id(za_account_id));
            }
        }
        // Mail spec 5.7: never a sealed source's hash, whatever the accounts
        // named; checked for every id before the first link is written.
        for blob_id in &request.blob_ids {
            if let MaybeInvalid::Value(blob_id) = blob_id {
                self.za_refuse_raw_source(blob_id).await?;
            }
        }

        for blob_id in request.blob_ids {
            let blob_id = match blob_id {
                MaybeInvalid::Value(blob_id) => blob_id,
                invalid => {
                    response.not_copied.append(
                        invalid,
                        SetError::new(SetErrorType::BlobNotFound).with_description(
                            "blobId does not exist or not enough permissions to access it.",
                        ),
                    );
                    continue;
                }
            };
            if self.has_access_blob(&blob_id, access_token).await? {
                // Enforce quota
                if !access_token.has_permission(Permission::UnlimitedUploads)
                    && !self
                        .blob_has_quota(account_id, 1)
                        .await
                        .caused_by(trc::location!())?
                        .allowed
                {
                    response.not_copied.append(
                        blob_id,
                        SetError::over_quota().with_description(format!(
                            "You have exceeded the blob quota of {} files or {} bytes.",
                            self.core.jmap.upload_tmp_quota_amount,
                            self.core.jmap.upload_tmp_quota_size
                        )),
                    );
                    continue;
                }

                let mut batch = BatchBuilder::new();
                let until = now() + self.core.jmap.upload_tmp_ttl;
                batch.with_account_id(account_id).set(
                    BlobOp::Link {
                        hash: blob_id.hash.clone(),
                        to: BlobLink::Temporary { until },
                    },
                    vec![],
                );
                self.store()
                    .write(batch.build_all())
                    .await
                    .caused_by(trc::location!())?;

                let dest_blob_id = BlobId {
                    hash: blob_id.hash.clone(),
                    class: BlobClass::Reserved {
                        account_id,
                        expires: until,
                    },
                    section: blob_id.section.clone(),
                };

                response.copied.append(blob_id, dest_blob_id);
            } else {
                response.not_copied.append(
                    blob_id,
                    SetError::new(SetErrorType::BlobNotFound).with_description(
                        "blobId does not exist or not enough permissions to access it.",
                    ),
                );
            }
        }

        Ok(response)
    }
}
