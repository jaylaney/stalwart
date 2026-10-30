/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Calendar spec 1.3 and 4.3, plan 1's outcome note: a key account is never
//! authenticated against an external directory. The request is refused
//! before the directory is contacted, and the directory synchronisation
//! never replaces a password credential that holds the marker.
//!
//! The directory is SQL on an SQLite file. Its login and recipient queries
//! are `INSERT ... SELECT ... RETURNING` statements, so every execution
//! that finds a row records the address it was asked for in `lookups`:
//! "the directory was never contacted" is the absence of a row, read after
//! the request returned, with no timing involved.
//!
//! Call sites in `route_auth_request` (`common::auth::authentication`):
//! - `za_refuse_directory_login` in the Basic arm, before
//!   `directory.authenticate`: a password login whose name resolves to a
//!   key account. Covered (`refusal`).
//! - `za_refuse_directory_account` in the Basic arm, after
//!   `directory.authenticate`: a login the directory accepts under a name
//!   that is not the key account's but maps to its address. Covered
//!   (`renamed_refusal`).
//! - `za_refuse_directory_account` in the Bearer arm: only a directory with
//!   bearer support (`Directory::has_bearer_token_support`, OIDC) reaches
//!   it, so it needs an OIDC provider and is not covered here.
//!
//! Two lookups synchronise a key account through a SQL directory with no
//! refusal, the recipient resolution (`rcpt_resolve`) and the recovery
//! admin's impersonation of a name only the directory knows
//! (`impersonated_account_id`); `merge_directory_secret` keeps the marker
//! through both (`sync_paths`).

use super::{STRONG, user_permissions};
use crate::utils::{account::Account, server::TestServer};
use common::{BuildServer, Server, auth::AuthRequest, network::RcptResolution};
use registry::schema::{
    prelude::{ObjectType, Property},
    structs::{self, Authentication, SqlAuthStore, SqlDirectory},
};
use std::net::IpAddr;
use store::{Rows, Store, backend::sqlite::SqliteStore};
use types::id::Id;
use vault::ZA_MARKER;

/// A key account whose address the directory also holds, under its own
/// password.
const KEY: &str = "ext-directory-key@example.com";
/// An ordinary account that exists only in the directory.
const PLAIN: &str = "ext-directory-plain@example.com";
/// A login only the directory knows; the directory maps it to `KEY`.
const RENAMED: &str = "ext-directory-renamed@example.com";

const KEY_DIRECTORY_SECRET: &str = "key directory secret 1";
const PLAIN_DIRECTORY_SECRET: &str = "plain directory secret 2";
const RENAMED_DIRECTORY_SECRET: &str = "renamed directory secret 3";

/// The reason `za_refuse_directory_login` gives.
const REFUSAL: &str = "Zero-access accounts cannot authenticate through an external directory";

/// Client address of every login in this module, so its deliberate
/// failures do not consume the loopback address's fail2ban budget.
const CLIENT_IP: [u8; 4] = [10, 0, 50, 1];

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access external directory tests...");
    let admin = test.account("admin@example.com").clone();
    // Logins through the recovery admin never reach a directory, so it edits
    // the registry while the directory serves every domain.
    let recovery = test.account("admin").clone();

    let key = admin
        .create_key_user_account(
            KEY,
            STRONG,
            "External Directory Key",
            &[],
            user_permissions(),
        )
        .await;
    let key_id = key.id();
    let before = registry_user(&test.server, key_id).await;
    assert_eq!(
        before.password_credential().map(|c| c.secret.as_str()),
        Some(ZA_MARKER)
    );

    let dir = format!("{}/ext-directory", test.tmp_dir());
    std::fs::create_dir_all(&dir).unwrap();
    let path = format!("{dir}/directory.sqlite");
    let sql = SqliteStore::open(structs::SqliteStore {
        path: path.clone(),
        pool_workers: None,
        pool_max_connections: 1,
    })
    .unwrap();
    populate(&sql).await;

    let directory_id = recovery
        .registry_create_object(structs::Directory::Sql(SqlDirectory {
            description: "Zero-access external directory test".to_string(),
            store: SqlAuthStore::Sqlite(structs::SqliteStore {
                path,
                ..Default::default()
            }),
            query_login: lookup_query("login"),
            query_recipient: lookup_query("recipient"),
            query_member_of: None,
            query_email_aliases: None,
            column_email: "name".to_string(),
            column_secret: "secret".to_string(),
            column_class: Some("type".to_string()),
            column_description: Some("description".to_string()),
            member_tenant_id: None,
        }))
        .await;
    recovery
        .registry_update_setting(
            Authentication {
                directory_id: Some(directory_id),
                ..Default::default()
            },
            &[Property::DirectoryId],
        )
        .await;
    recovery.reload_settings().await;
    // `test.server` keeps the core it was built with; the reload replaced it.
    let server = test.server.inner.build_server();
    assert!(
        server.get_default_directory().is_some(),
        "the SQL directory serves every domain"
    );

    let plain_id = positive_control(&server, &sql).await;
    refusal(&server, &sql, key_id, &before).await;
    renamed_refusal(&server, &sql, key_id, &before).await;
    sync_paths(&server, &sql, &recovery, key_id, &before).await;

    // Take the directory away again, then the marker and the vault password.
    recovery
        .registry_update_setting(
            Authentication {
                directory_id: None,
                ..Default::default()
            },
            &[Property::DirectoryId],
        )
        .await;
    recovery
        .registry_destroy(ObjectType::Directory, [directory_id])
        .await
        .assert_destroyed(&[directory_id]);
    recovery.reload_settings().await;
    drop(server);
    let server = test.server.inner.build_server();
    assert!(server.get_default_directory().is_none());
    marker_survives(&server, &sql, key_id).await;

    recovery
        .registry_destroy(ObjectType::Account, [plain_id])
        .await
        .assert_destroyed(&[plain_id]);
    admin.destroy_account(key).await;
    test.wait_for_tasks().await;
    drop(sql);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Case 1: an ordinary account that exists only in the directory logs in
/// through it, and its login is recorded. Without this the refusals' empty
/// records below would prove nothing.
async fn positive_control(server: &Server, sql: &Store) -> Id {
    println!("positive_control");
    assert_eq!(
        server.account_id_from_email(PLAIN, false).await.unwrap(),
        None,
        "the registry does not know the directory's account yet"
    );
    let token = server
        .authenticate(&request(PLAIN, PLAIN_DIRECTORY_SECRET))
        .await
        .unwrap_or_else(|err| panic!("the directory's account did not log in: {err:?}"));
    assert_eq!(
        lookups(sql).await,
        [("login", PLAIN)],
        "the directory authenticated the login"
    );
    assert_eq!(
        server.account_id_from_email(PLAIN, false).await.unwrap(),
        Some(token.account_id()),
        "the login synchronised the directory's account into the registry"
    );
    Id::from(token.account_id())
}

/// Case 2 (`za_refuse_directory_login`): the key account's own address,
/// with the directory's password and with the vault password, is refused
/// before the directory runs a query.
async fn refusal(server: &Server, sql: &Store, key_id: Id, before: &structs::UserAccount) {
    println!("refusal");
    for secret in [KEY_DIRECTORY_SECRET, STRONG] {
        assert_refused(server, KEY, secret).await;
    }
    assert_eq!(
        lookups(sql).await,
        [("login", PLAIN)],
        "the directory was contacted for a key account"
    );
    assert_eq!(
        &registry_user(server, key_id).await,
        before,
        "a refused login changed the key account"
    );
}

/// Case 3 (`za_refuse_directory_account` in the Basic arm): the directory
/// accepts a login under a name the registry does not hold and maps it to
/// the key account's address. The directory is contacted (it is what names
/// the account), but the request is refused before the synchronisation.
async fn renamed_refusal(server: &Server, sql: &Store, key_id: Id, before: &structs::UserAccount) {
    println!("renamed_refusal");
    assert_refused(server, RENAMED, RENAMED_DIRECTORY_SECRET).await;
    assert_eq!(
        lookups(sql).await,
        [("login", PLAIN), ("login", RENAMED)],
        "the directory authenticated the renamed login"
    );
    assert_eq!(
        &registry_user(server, key_id).await,
        before,
        "the renamed login synchronised the directory's row into the key account"
    );
    assert_eq!(
        server.account_id_from_email(RENAMED, false).await.unwrap(),
        None
    );
}

/// Case 4: the lookups that synchronise a key account through the
/// directory. Neither is refused (the recipient is the key account's, and
/// an impersonation token is refused at use, mail ruling R35); the
/// directory's secret differs from the marker each time and never replaces
/// it.
async fn sync_paths(
    server: &Server,
    sql: &Store,
    recovery: &Account,
    key_id: Id,
    before: &structs::UserAccount,
) {
    println!("sync_paths");
    // The recipient lookup consults the directory before the registry.
    assert_eq!(
        server.rcpt_resolve(KEY, false, 0).await.unwrap(),
        RcptResolution::Accept
    );
    assert_eq!(
        lookups(sql).await,
        [("login", PLAIN), ("login", RENAMED), ("recipient", KEY)],
        "the recipient lookup reached the directory"
    );
    assert_eq!(
        &registry_user(server, key_id).await,
        before,
        "the recipient's synchronisation changed the key account"
    );

    // The recovery admin impersonates a name only the directory knows; the
    // directory maps it to the key account, whose description the
    // synchronisation may rewrite but whose credentials it keeps.
    let token = server
        .authenticate(&request(
            &format!("{RENAMED}%{}", recovery.name()),
            recovery.secret(),
        ))
        .await
        .unwrap_or_else(|err| panic!("the impersonation failed: {err:?}"));
    assert_eq!(Id::from(token.account_id()), key_id);
    assert!(token.za_is_impersonated());
    assert!(token.session_keys().is_none());
    assert_eq!(
        lookups(sql).await,
        [
            ("login", PLAIN),
            ("login", RENAMED),
            ("recipient", KEY),
            ("recipient", RENAMED)
        ],
        "the impersonation's lookup reached the directory"
    );
    assert_eq!(
        registry_user(server, key_id).await.credentials,
        before.credentials,
        "the impersonation's synchronisation replaced a credential"
    );
}

/// Case 5: with the directory gone, the registry credential is still the
/// marker, the vault password logs in with keys, and the directory's
/// password does not.
async fn marker_survives(server: &Server, sql: &Store, key_id: Id) {
    println!("marker_survives");
    let user = registry_user(server, key_id).await;
    assert_eq!(
        user.password_credential().map(|c| c.secret.as_str()),
        Some(ZA_MARKER)
    );
    let (token, keys) = server
        .authenticate_with_keys(&request(KEY, STRONG))
        .await
        .unwrap_or_else(|err| panic!("the vault password did not log in: {err:?}"));
    assert_eq!(Id::from(token.account_id()), key_id);
    assert!(keys.is_some(), "the vault password unlocks the keys");
    assert!(
        server
            .authenticate(&request(KEY, KEY_DIRECTORY_SECRET))
            .await
            .is_err(),
        "the directory's password logs in to the key account"
    );
    assert!(
        !lookups(sql)
            .await
            .iter()
            .any(|&(query, address)| query == "login" && address == KEY),
        "the directory was asked to log the key account in"
    );
}

async fn assert_refused(server: &Server, name: &str, secret: &str) {
    match server.authenticate(&request(name, secret)).await {
        Ok(token) => panic!(
            "{name} authenticated through the directory as account {}",
            token.account_id()
        ),
        Err(err) => {
            assert!(
                err.matches(trc::EventType::Auth(trc::AuthEvent::Failed)),
                "{name}: {err:?}"
            );
            assert_eq!(
                err.value_as_str(trc::Key::Reason),
                Some(REFUSAL),
                "{name}: {err:?}"
            );
        }
    }
}

fn request(name: &str, secret: &str) -> AuthRequest {
    AuthRequest::from_plain(name, secret, 0, IpAddr::from(CLIENT_IP))
}

async fn registry_user(server: &Server, id: Id) -> structs::UserAccount {
    server
        .registry()
        .object::<structs::Account>(id)
        .await
        .unwrap()
        .and_then(|account| account.into_user())
        .expect("a user account")
}

/// A directory query that records the address it was asked for, with the
/// row it found.
fn lookup_query(kind: &str) -> String {
    format!(
        concat!(
            "INSERT INTO lookups (query, address, name, secret, description, type) ",
            "SELECT '{}', login, name, secret, description, type FROM accounts ",
            "WHERE login = $1 RETURNING name, secret, description, type"
        ),
        kind
    )
}

async fn populate(sql: &Store) {
    for query in [
        concat!(
            "CREATE TABLE accounts (login TEXT PRIMARY KEY, name TEXT NOT NULL, ",
            "secret TEXT, description TEXT, type TEXT NOT NULL)"
        ),
        concat!(
            "CREATE TABLE lookups (seq INTEGER PRIMARY KEY AUTOINCREMENT, ",
            "query TEXT NOT NULL, address TEXT NOT NULL, name TEXT, secret TEXT, ",
            "description TEXT, type TEXT)"
        ),
    ] {
        sql.sql_query::<usize>(query, vec![]).await.unwrap();
    }
    for (login, name, secret, description) in [
        (PLAIN, PLAIN, PLAIN_DIRECTORY_SECRET, None),
        (KEY, KEY, KEY_DIRECTORY_SECRET, None),
        (
            RENAMED,
            KEY,
            RENAMED_DIRECTORY_SECRET,
            Some("Renamed by the directory"),
        ),
    ] {
        sql.sql_query::<usize>(
            concat!(
                "INSERT INTO accounts (login, name, secret, description, type) ",
                "VALUES ($1, $2, $3, $4, 'individual')"
            ),
            vec![
                login.into(),
                name.into(),
                secret.into(),
                description.map_or(store::Value::Null, Into::into),
            ],
        )
        .await
        .unwrap();
    }
}

async fn lookups(sql: &Store) -> Vec<(&'static str, &'static str)> {
    let known = |value: &store::Value<'_>| -> &'static str {
        let value = value.to_str();
        ["login", "recipient", KEY, PLAIN, RENAMED]
            .into_iter()
            .find(|known| *known == value.as_ref())
            .unwrap_or_else(|| panic!("unexpected lookup value {value}"))
    };
    sql.sql_query::<Rows>("SELECT query, address FROM lookups ORDER BY seq", vec![])
        .await
        .unwrap()
        .rows
        .iter()
        .map(|row| (known(&row.values[0]), known(&row.values[1])))
        .collect()
}
