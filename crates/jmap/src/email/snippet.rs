/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

<<<<<<< HEAD
use crate::email::za::za_refuse_snippet_terms;
use common::{Server, auth::AccessToken};
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    message::{
        metadata::{
            ArchivedMetadataPartType, DecodedPartContent, MessageMetadata, MetadataHeaderName,
        },
        za::view::{ZaMailView, za_readable},
=======
use common::{
    Server,
    auth::AccessToken,
    storage::batch::{DocumentBatch, Lookup, MAX_BATCH_BYTES, MAX_BATCH_DOCUMENTS},
};
use email::{
    cache::{
        MessageCacheFetch,
        email::{MessageAccess, MessageCacheAccess},
        mailbox::MailboxCacheAccess,
    },
    message::{
        messagedata::{EmailMessageData, MetadataForm},
        metadata::{PartKind, PartSource},
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
    },
};
use jmap_proto::{
    method::{
        query::Filter,
        search_snippet::{GetSearchSnippetRequest, GetSearchSnippetResponse, SearchSnippet},
    },
    object::email::EmailFilter,
    request::MaybeInvalid,
};
use nlp::language::{Language, search_snippet::generate_snippet, stemmer::Stemmer};
use std::{borrow::Cow, future::Future};
use store::{
    backend::MAX_TOKEN_LENGTH,
    search::{MAX_PLANNED_TOKENS, MAX_QUERY_LEAVES, MAX_QUERY_PLANNED_TOKENS},
};
use trc::AddContext;
use types::{acl::Acl, collection::Collection};
use utils::cheeky_hash::{CheekyHash, CheekyHashSet};

pub trait EmailSearchSnippet: Sync + Send {
    fn email_search_snippet(
        &self,
        request: GetSearchSnippetRequest,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<GetSearchSnippetResponse>> + Send;
}

impl EmailSearchSnippet for Server {
    async fn email_search_snippet(
        &self,
        request: GetSearchSnippetRequest,
        access_token: &AccessToken,
    ) -> trc::Result<GetSearchSnippetResponse> {
        // Mail spec 8: snippets mark terms in sealed content, which a search
        // cannot find; a key account's request carrying a text, subject or
        // body property is refused before any message is read, whatever the
        // property tokenises to.
        za_refuse_snippet_terms(self, request.account_id.document_id(), &request.filter).await?;
        let mut filter_stack = vec![];
        let mut include_term = true;
        let mut terms = vec![];
        let mut seen = CheekyHashSet::default();
        let mut planned = 0usize;
        let mut conditions = 0usize;
        let mut is_exact = false;
        let mut language = self.core.email.default_language;

        for cond in request.filter {
            match cond {
                Filter::Property(cond) => {
                    if let EmailFilter::Text(text)
                    | EmailFilter::Subject(text)
                    | EmailFilter::Body(text) = cond
                        && include_term
                    {
                        conditions += 1;
                        if conditions > MAX_QUERY_LEAVES {
                            return Err(too_many_terms());
                        }
                        let (text, language_) =
                            Language::detect(text, self.core.email.default_language);
                        language = language_;
                        if (text.starts_with('"') && text.ends_with('"'))
                            || (text.starts_with('\'') && text.ends_with('\''))
                        {
                            let before = terms.len();
                            terms.extend(
                                language
                                    .tokenize_text(&text, MAX_TOKEN_LENGTH)
                                    .take(MAX_PLANNED_TOKENS)
                                    .map(|token| token.word.into_owned()),
                            );
                            planned += terms.len() - before;
                            is_exact = true;
                        } else {
                            let mut condition_words = 0;
                            for token in Stemmer::new(&text, language, MAX_TOKEN_LENGTH) {
                                if !seen.insert(CheekyHash::new(token.word.as_bytes())) {
                                    continue;
                                }
                                terms.push(token.word.into_owned());
                                if let Some(stemmed_word) = token.stemmed_word
                                    && seen.insert(CheekyHash::new(stemmed_word.as_bytes()))
                                {
                                    terms.push(stemmed_word.into_owned());
                                }
                                condition_words += 1;
                                if condition_words == MAX_PLANNED_TOKENS {
                                    break;
                                }
                            }
                            planned += condition_words;
                        }
                        if planned > MAX_QUERY_PLANNED_TOKENS {
                            return Err(too_many_terms());
                        }
                    }
                }
                Filter::And | Filter::Or => {
                    filter_stack.push(cond);
                }
                Filter::Not => {
                    filter_stack.push(cond);
                    include_term = !include_term;
                }
                Filter::Close => {
                    if matches!(filter_stack.pop(), Some(Filter::Not)) {
                        include_term = !include_term;
                    }
                }
            }
        }
        let email_ids = request.email_ids.unwrap();
        if email_ids.len() > self.core.jmap.snippet_max_results {
            return Err(trc::JmapEvent::RequestTooLarge.into_err());
        }

        let account_id = request.account_id.document_id();
        let cached_messages = self
            .get_cached_messages(account_id)
            .await
            .caused_by(trc::location!())?;
        let access = if access_token.is_member(account_id) {
            MessageAccess::All
        } else {
            MessageAccess::Mailboxes(cached_messages.shared_mailboxes(access_token, Acl::ReadItems))
        };

        let mut response = GetSearchSnippetResponse {
            account_id: request.account_id,
            list: Vec::with_capacity(email_ids.len()),
            not_found: None,
        };
        let mut not_found = Vec::new();
        let mut batch = DocumentBatch::new(MAX_BATCH_BYTES);

<<<<<<< HEAD
        if email_ids.len() > self.core.jmap.snippet_max_results {
            return Err(trc::JmapEvent::RequestTooLarge.into_err());
        }
        // Mail spec 7, 8: the account's keys, once for the request; without
        // terms nothing is read. Defence in depth: a key account's request
        // with terms is refused above, so this branch and the sealed reads
        // below are unreachable for one.
        let keys = if terms.is_empty() {
            None
        } else {
            self.za_mail_keys(access_token, account_id).await?
        };

        for email_id in email_ids {
            let email_id = match email_id {
                MaybeInvalid::Value(email_id) => email_id,
                invalid => {
                    not_found.push(invalid);
                    continue;
                }
            };
            let document_id = email_id.document_id();
            let mut snippet = SearchSnippet {
                email_id,
                subject: None,
                preview: None,
            };
            if !document_ids.contains(document_id) {
                not_found.push(MaybeInvalid::Value(email_id));
                continue;
            } else if terms.is_empty() {
                response.list.push(snippet);
                continue;
            }
            let metadata_ = match self
                .store()
                .get_value::<Archive<AlignedBytes>>(ValueKey::property(
                    account_id,
                    Collection::Email,
                    document_id,
                    EmailField::Metadata,
                ))
                .await?
            {
                Some(metadata) => metadata,
                None => {
                    not_found.push(MaybeInvalid::Value(email_id));
                    continue;
                }
            };
            // Mail spec 7, 10: the unsealed view; an unreadable message is
            // not found.
            let Some(view) = za_readable(
                self.za_metadata_view(&metadata_, keys.as_deref(), account_id, document_id)
                    .await,
            )?
            else {
                not_found.push(MaybeInvalid::Value(email_id));
                continue;
            };
            let metadata = view
                .unarchive::<MessageMetadata>()
                .caused_by(trc::location!())?;

            // Add subject snippet
            let contents = &metadata.contents[0];
            if let Some(subject) = contents
                .root_part()
                .header_value(&MetadataHeaderName::Subject)
                .and_then(|v| v.as_text())
                .and_then(|v| generate_snippet(v, &terms, language, is_exact))
            {
                snippet.subject = subject.into();
            }

            // Download message
            let raw_body = if let Some(raw_body) = self
                .blob_store()
                .get_blob(metadata.blob_hash.0.as_slice(), 0..usize::MAX)
                .await?
            {
                let Some(raw_body) = za_readable(
                    self.za_open_message_blob(
                        matches!(view, Cow::Owned(_)),
                        keys.as_deref(),
                        account_id,
                        document_id,
                        raw_body,
                    )
                    .await,
                )?
                else {
                    not_found.push(MaybeInvalid::Value(email_id));
                    continue;
                };
                raw_body
            } else {
                trc::event!(
                    Store(trc::StoreEvent::NotFound),
                    AccountId = account_id,
                    DocumentId = email_id.document_id(),
                    Collection = Collection::Email,
                    BlobId = metadata.blob_hash.0.as_slice(),
                    Details = "Blob not found.",
                    CausedBy = trc::location!(),
                );
=======
        for chunk in email_ids.chunks(MAX_BATCH_DOCUMENTS) {
            batch.reset();
            if !terms.is_empty() {
                for email_id in chunk {
                    if let MaybeInvalid::Value(email_id) = email_id
                        && cached_messages
                            .email_by_id(&email_id.document_id())
                            .is_some_and(|message| access.allows(message))
                    {
                        batch.request(email_id.document_id());
                    }
                }
            }

            loop {
                self.read_metadata_batch(account_id, MetadataForm::Structure, &mut batch)
                    .await
                    .caused_by(trc::location!())?;
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184

                for email_id in chunk {
                    let email_id = match email_id {
                        MaybeInvalid::Value(email_id) => *email_id,
                        invalid => {
                            if batch.is_first_round() {
                                not_found.push(invalid.clone());
                            }
                            continue;
                        }
                    };
                    let document_id = email_id.document_id();
                    let mut snippet = SearchSnippet {
                        email_id,
                        subject: None,
                        preview: None,
                    };
                    if !cached_messages
                        .email_by_id(&document_id)
                        .is_some_and(|message| access.allows(message))
                    {
                        if batch.is_first_round() {
                            not_found.push(MaybeInvalid::Value(email_id));
                        }
                        continue;
                    } else if terms.is_empty() {
                        response.list.push(snippet);
                        continue;
                    }
                    let structure = match batch.lookup(document_id) {
                        Lookup::Found(structure) => structure,
                        Lookup::Missing => {
                            not_found.push(MaybeInvalid::Value(email_id));
                            continue;
                        }
                        Lookup::Skip => continue,
                    };
                    let metadata = structure.unarchive().caused_by(trc::location!())?;
                    let root = metadata.root();

                    // Add subject snippet
                    if let Some(subject) = root
                        .envelope()
                        .subject()
                        .and_then(|subject| generate_snippet(subject, &terms, language, is_exact))
                    {
                        snippet.subject = subject.into();
                    }

                    // Download message
                    let blob_hash = metadata.blob_hash();
                    let Some(raw_body) = self
                        .fetch_blob(&blob_hash, 0..usize::MAX, Some(metadata.size() as u64))
                        .await?
                    else {
                        trc::event!(
                            Store(trc::StoreEvent::NotFound),
                            AccountId = account_id,
                            DocumentId = email_id.document_id(),
                            Collection = Collection::Email,
                            BlobId = blob_hash.as_slice(),
                            Details = "Blob not found.",
                            CausedBy = trc::location!(),
                        );

                        not_found.push(MaybeInvalid::Value(email_id));
                        continue;
                    };
                    let raw = metadata.raw_message(None, &raw_body);
                    let source = PartSource::Raw(raw);

                    // Find a matching part
                    'outer: for part in root.parts() {
                        match part.kind() {
                            PartKind::Text | PartKind::Html => {
                                if let Some(body) = part.plain_text(&source).and_then(|text| {
                                    generate_snippet(&text, &terms, language, is_exact)
                                }) {
                                    snippet.preview = body.into();
                                    break;
                                }
                            }
                            PartKind::Message => {
                                let Some(nested) = part.nested() else {
                                    continue;
                                };
                                let Some(nested_source) = metadata.source(nested, raw) else {
                                    continue;
                                };
                                for part in nested.parts().filter(|part| part.is_text()) {
                                    if let Some(body) =
                                        part.plain_text(&nested_source).and_then(|text| {
                                            generate_snippet(&text, &terms, language, is_exact)
                                        })
                                    {
                                        snippet.preview = body.into();
                                        break 'outer;
                                    }
                                }
                            }
                            _ => (),
                        }
                    }

                    response.list.push(snippet);
                }

                if !batch.next_round() {
                    break;
                }
            }
        }

        if !not_found.is_empty() {
            response.not_found = Some(not_found);
        }

        Ok(response)
    }
}

#[cold]
fn too_many_terms() -> trc::Error {
    trc::LimitEvent::SizeRequest
        .into_err()
        .details("Too many search terms in one query")
}
