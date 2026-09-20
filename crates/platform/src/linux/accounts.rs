//! Real Linux login accounts. AccountsService owns all privileged writes.
use super::{authorization, dbus, identity::LinuxUserContext};
use crate::service::ServiceError;
use std::sync::Arc;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};

const SERVICE: &str = "org.freedesktop.Accounts";
const PATH: &str = "/org/freedesktop/Accounts";
const USER: &str = "org.freedesktop.Accounts.User";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub uid: u64,
    pub username: String,
    pub display_name: String,
    pub admin: bool,
    pub locked: bool,
    pub system: bool,
    pub local: bool,
    path: OwnedObjectPath,
}

pub struct Accounts {
    connection: Connection,
    current_uid: u64,
    interaction: Option<Arc<dyn authorization::Interaction>>,
}

impl Accounts {
    /// Optional startup metadata must not wait for an interactive authorization timeout.
    pub fn current_account() -> Result<Account, ServiceError> {
        let context = LinuxUserContext::current().map_err(|_| ServiceError::PermissionDenied)?;
        let connection = zbus::blocking::connection::Builder::system()
            .map_err(map_error)?
            .method_timeout(std::time::Duration::from_secs(2))
            .build()
            .map_err(map_error)?;
        Self {
            connection,
            current_uid: u64::from(context.process.uid),
            interaction: None,
        }
        .actor()
    }

    pub fn current() -> Result<Self, ServiceError> {
        let context = LinuxUserContext::current().map_err(|_| ServiceError::PermissionDenied)?;
        Ok(Self {
            connection: dbus::authorization_system()?,
            current_uid: u64::from(context.process.uid),
            interaction: None,
        })
    }

    pub fn set_interaction(&mut self, interaction: Arc<dyn authorization::Interaction>) {
        self.interaction = Some(interaction);
    }

    fn manager(&self) -> Result<Proxy<'_>, ServiceError> {
        Proxy::new(&self.connection, SERVICE, PATH, SERVICE).map_err(map_error)
    }

    fn proxy<'a>(&'a self, account: &'a Account) -> Result<Proxy<'a>, ServiceError> {
        Proxy::new(&self.connection, SERVICE, account.path.as_str(), USER).map_err(map_error)
    }

    fn read(&self, path: OwnedObjectPath) -> Result<Account, ServiceError> {
        let proxy =
            Proxy::new(&self.connection, SERVICE, path.as_str(), USER).map_err(map_error)?;
        Ok(Account {
            uid: proxy.get_property("Uid").map_err(map_error)?,
            username: proxy.get_property("UserName").map_err(map_error)?,
            display_name: proxy.get_property("RealName").map_err(map_error)?,
            admin: proxy
                .get_property::<i32>("AccountType")
                .map_err(map_error)?
                == 1,
            locked: proxy.get_property("Locked").map_err(map_error)?,
            system: proxy.get_property("SystemAccount").map_err(map_error)?,
            local: proxy.get_property("LocalAccount").map_err(map_error)?,
            path: path.clone(),
        })
    }

    pub fn actor(&self) -> Result<Account, ServiceError> {
        let path = self
            .manager()?
            .call("FindUserById", &(self.current_uid as i64))
            .map_err(map_error)?;
        let account = self.read(path)?;
        if account.uid != self.current_uid {
            return Err(ServiceError::PermissionDenied);
        }
        Ok(account)
    }

    /// Ordinary users never enumerate other accounts, even when polkit would allow it.
    pub fn visible(&self) -> Result<Vec<Account>, ServiceError> {
        let actor = self.actor()?;
        if !actor.admin {
            return Ok(vec![actor]);
        }
        let paths: Vec<OwnedObjectPath> = self
            .manager()?
            .call("ListCachedUsers", &())
            .map_err(map_error)?;
        let mut accounts = vec![actor.clone()];
        for path in paths {
            let account = self.read(path)?;
            if account.uid != actor.uid && account.uid != 0 && !account.system && account.local {
                accounts.push(account);
            }
        }
        accounts.sort_by(|a, b| a.username.cmp(&b.username));
        Ok(accounts)
    }

    fn target(
        &self,
        username: &str,
        admin_only: bool,
        destructive: bool,
    ) -> Result<Account, ServiceError> {
        let actor = self.actor()?;
        let target = self
            .visible()?
            .into_iter()
            .find(|a| a.username == username)
            .ok_or(ServiceError::PermissionDenied)?;
        authorize(&actor, &target, admin_only, destructive)?;
        Ok(target)
    }

    fn prepare(&self, own: bool) -> Result<Option<authorization::Lease>, ServiceError> {
        authorization::prepare_using(
            &self.connection,
            &self.connection,
            if own {
                authorization::Action::ChangeOwnAccount
            } else {
                authorization::Action::ManageAccounts
            },
            self.interaction.clone(),
        )
    }

    pub fn rename(&self, username: &str, display_name: &str) -> Result<(), ServiceError> {
        let target = self.target(username, false, false)?;
        let _lease = self.prepare(target.uid == self.current_uid)?;
        self.proxy(&target)?
            .call("SetRealName", &(display_name,))
            .map_err(map_error)
    }

    pub fn password(&self, username: &str, password: &str) -> Result<(), ServiceError> {
        let target = self.target(username, false, false)?;
        if target.uid == self.current_uid {
            return self
                .interaction
                .as_ref()
                .ok_or(ServiceError::Unsupported)?
                .change_own_password();
        }
        let hash = password_hash(password)?;
        let _lease = self.prepare(false)?;
        self.proxy(&target)?
            .call("SetPassword", &(hash.as_str(), ""))
            .map_err(map_error)
    }

    pub fn set_locked(&self, username: &str, locked: bool) -> Result<(), ServiceError> {
        let target = self.target(username, true, locked)?;
        let _lease = self.prepare(false)?;
        self.proxy(&target)?
            .call("SetLocked", &(locked,))
            .map_err(map_error)
    }

    pub fn set_admin(&self, username: &str, admin: bool) -> Result<(), ServiceError> {
        let target = self.target(username, true, !admin)?;
        let _lease = self.prepare(false)?;
        self.proxy(&target)?
            .call("SetAccountType", &(i32::from(admin),))
            .map_err(map_error)
    }

    pub fn delete(&self, username: &str) -> Result<(), ServiceError> {
        let target = self.target(username, true, true)?;
        let _lease = self.prepare(false)?;
        // Never remove the user's home or files.
        self.manager()?
            .call("DeleteUser", &(target.uid as i64, false))
            .map_err(map_error)
    }

    pub fn create(
        &self,
        username: &str,
        display_name: &str,
        admin: bool,
        password: &str,
    ) -> Result<Account, ServiceError> {
        let actor = self.actor()?;
        if !actor.admin {
            return Err(ServiceError::PermissionDenied);
        }
        let hash = password_hash(password)?;
        let _lease = self.prepare(false)?;
        let path = self
            .manager()?
            .call("CreateUser", &(username, display_name, i32::from(admin)))
            .map_err(map_error)?;
        let mut account = self
            .read(path)
            .map_err(|_| ServiceError::AccountPasswordSetupFailed)?;
        // Do not delete an account after partial failure: it and its files now exist.
        // The caller reports that setting its password failed and refreshes the list.
        self.proxy(&account)?
            .call::<_, _, ()>("SetPassword", &(hash.as_str(), ""))
            .map_err(|_| ServiceError::AccountPasswordSetupFailed)?;
        account.locked = false;
        Ok(account)
    }
}

fn authorize(
    actor: &Account,
    target: &Account,
    admin_only: bool,
    destructive: bool,
) -> Result<(), ServiceError> {
    if !target.local
        || target.system
        || target.uid == 0
        || ((!actor.admin) && (admin_only || actor.uid != target.uid))
        || (destructive && actor.uid == target.uid)
    {
        return Err(ServiceError::PermissionDenied);
    }
    Ok(())
}

fn map_error(error: zbus::Error) -> ServiceError {
    if let zbus::Error::MethodError(name, _, _) = &error
        && name.as_str() == "org.freedesktop.Accounts.Error.PermissionDenied"
    {
        return ServiceError::PermissionDenied;
    }
    dbus::map_error(error)
}

#[path = "accounts_password.rs"]
mod password;
use password::password_hash;

#[cfg(test)]
#[path = "../../tests/unit/linux/accounts_dbus_tests.rs"]
mod dbus_tests;

#[cfg(test)]
#[path = "../../tests/unit/linux/accounts/tests.rs"]
mod tests;
