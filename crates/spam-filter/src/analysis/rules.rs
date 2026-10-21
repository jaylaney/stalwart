/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::future::Future;

use common::{
    Server,
    config::mailstore::spamfilter::{IpResolver, Location},
    expr::{Variable, functions::ResolveVariable, if_block::IfBlock},
};

use crate::{
    SpamFilterContext, TextPart,
    modules::expression::{EmailHeader, SpamFilterResolver, StringResolver},
};

/// Mail spec 10: a rule evaluated for a key account's message does not trace
/// its result, which can echo a header, the body or an address.
pub(crate) async fn eval<'x, R: TryFrom<Variable<'x>>, V: ResolveVariable>(
    server: &'x Server,
    ctx: &SpamFilterContext<'_>,
    if_block: &'x IfBlock,
    resolver: &'x V,
) -> Option<R> {
    if ctx.input.za_redact {
        server
            .za_eval_if(if_block, resolver, ctx.input.span_id)
            .await
    } else {
        server.eval_if(if_block, resolver, ctx.input.span_id).await
    }
}

pub trait SpamFilterAnalyzeRules: Sync + Send {
    fn spam_filter_analyze_rules(
        &self,
        ctx: &mut SpamFilterContext<'_>,
    ) -> impl Future<Output = ()> + Send;
}

impl SpamFilterAnalyzeRules for Server {
    async fn spam_filter_analyze_rules(&self, ctx: &mut SpamFilterContext<'_>) {
        if !self.core.spam.rules.url.is_empty() {
            for url in &ctx.output.urls {
                for rule in &self.core.spam.rules.url {
                    if let Some(tag) = eval::<String, _>(
                        self,
                        ctx,
                        rule,
                        &SpamFilterResolver::new(ctx, &url.element, url.location),
                    )
                    .await
                    {
                        ctx.result.tags.insert(tag);
                    }
                }
            }
        }

        if !self.core.spam.rules.domain.is_empty() {
            for domain in &ctx.output.domains {
                let resolver = StringResolver(domain.element.as_str());

                for rule in &self.core.spam.rules.domain {
                    if let Some(tag) = eval::<String, _>(
                        self,
                        ctx,
                        rule,
                        &SpamFilterResolver::new(ctx, &resolver, domain.location),
                    )
                    .await
                    {
                        ctx.result.tags.insert(tag);
                    }
                }
            }
        }

        if !self.core.spam.rules.email.is_empty() {
            for email in &ctx.output.emails {
                for rule in &self.core.spam.rules.email {
                    if let Some(tag) = eval::<String, _>(
                        self,
                        ctx,
                        rule,
                        &SpamFilterResolver::new(ctx, &email.element, email.location),
                    )
                    .await
                    {
                        ctx.result.tags.insert(tag);
                    }
                }
            }

            for (rcpt, location) in [
                (&ctx.output.recipients_to, Location::HeaderTo),
                (&ctx.output.recipients_cc, Location::HeaderCc),
                (&ctx.output.recipients_bcc, Location::HeaderBcc),
            ] {
                for email in rcpt {
                    for rule in &self.core.spam.rules.email {
                        if let Some(tag) = eval::<String, _>(
                            self,
                            ctx,
                            rule,
                            &SpamFilterResolver::new(ctx, email, location),
                        )
                        .await
                        {
                            ctx.result.tags.insert(tag);
                        }
                    }
                }
            }
        }

        if !self.core.spam.rules.ip.is_empty() {
            for ip in &ctx.output.ips {
                let ip_resolver = IpResolver::new(ip.element);

                for rule in &self.core.spam.rules.ip {
                    if let Some(tag) = eval::<String, _>(
                        self,
                        ctx,
                        rule,
                        &SpamFilterResolver::new(ctx, &ip_resolver, ip.location),
                    )
                    .await
                    {
                        ctx.result.tags.insert(tag);
                    }
                }
            }
        }

        if !self.core.spam.rules.header.is_empty() {
            for header in ctx.input.message.headers() {
                let raw = String::from_utf8_lossy(
                    ctx.input
                        .message
                        .raw_message()
                        .get(header.offset_start as usize..header.offset_end as usize)
                        .unwrap_or_default(),
                );
                let header_resolver = EmailHeader {
                    header,
                    raw: raw.as_ref(),
                };

                for rule in &self.core.spam.rules.header {
                    if let Some(tag) = eval::<String, _>(
                        self,
                        ctx,
                        rule,
                        &SpamFilterResolver::new(ctx, &header_resolver, Location::BodyText),
                    )
                    .await
                    {
                        ctx.result.tags.insert(tag);
                    }
                }
            }
        }

        if !self.core.spam.rules.body.is_empty() {
            for (idx, part) in ctx.output.text_parts.iter().enumerate() {
                let text = match part {
                    TextPart::Plain { text_body, .. } => *text_body,
                    TextPart::Html { text_body, .. } => text_body.as_str(),
                    TextPart::None => continue,
                };
                let idx = idx as u32;
                let location = if ctx.input.message.text_body.contains(&idx) {
                    Location::BodyText
                } else if ctx.input.message.html_body.contains(&idx) {
                    Location::BodyHtml
                } else {
                    Location::Attachment
                };
                let string_resolver = StringResolver(text);

                for rule in &self.core.spam.rules.body {
                    if let Some(tag) = eval::<String, _>(
                        self,
                        ctx,
                        rule,
                        &SpamFilterResolver::new(ctx, &string_resolver, location),
                    )
                    .await
                    {
                        ctx.result.tags.insert(tag);
                    }
                }
            }
        }

        if !self.core.spam.rules.any.is_empty() {
            let dummy_resolver = StringResolver("");
            for rule in &self.core.spam.rules.any {
                if let Some(tag) = eval::<String, _>(
                    self,
                    ctx,
                    rule,
                    &SpamFilterResolver::new(ctx, &dummy_resolver, Location::BodyText),
                )
                .await
                {
                    ctx.result.tags.insert(tag);
                }
            }
        }
    }
}
