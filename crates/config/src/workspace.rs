//! A loaded workspace seen from the folder rant runs in, which names, node paths and path
//! params resolve from. Commands plan, list and edit services through it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use hcl::Value;

use crate::build_plan::{self, BuildPlan};
use crate::diag::Diag;
use crate::group::{GroupDef, GroupFile, Groups};
use crate::model::{Model, NodeType};
use crate::param::{self, Param};
use crate::paths;
use crate::plan::{self, Named, Plan};
use crate::refs;
use crate::service::{self, Checked, Service};
use crate::settings::Settings;

pub struct Workspace {
    pub model: Model,
    here: PathBuf,
}

/// Param values in the order the command line gives them.
type Given = Vec<(String, Value)>;

/// A group, the params it takes, own and exposed, and the groups it includes by full name.
#[derive(Debug)]
pub struct GroupInfo {
    pub file: GroupFile,
    pub description: Option<String>,
    pub params: Vec<Param>,
    pub includes: Vec<String>,
}

/// The plan for a group, with every param it resolved, defaults included.
#[derive(Debug)]
pub struct GroupPlan {
    pub group: String,
    pub plan: Plan,
    pub values: BTreeMap<String, Value>,
}

/// A group file and the shortest name that finds it from here.
#[derive(Debug)]
pub struct GroupName<'a> {
    pub file: &'a GroupFile,
    pub short: String,
}

/// A service block, and what checking it against its group found.
#[derive(Debug)]
pub struct ServiceCheck<'a> {
    pub service: &'a Service,
    pub checked: Result<Checked, Diag>,
}

impl Workspace {
    /// The packages of these settings seen from `here`, and every error loading met.
    pub fn load(settings: Settings, here: &Path, rant_home: &Path) -> (Workspace, Vec<Diag>) {
        let (model, diags) = Model::load(settings, rant_home);
        (Workspace { model, here: paths::normalize(here) }, diags)
    }

    /// The node type a reference names, in any form refs takes.
    pub fn resolve(&self, reference: &str) -> Result<NodeType, String> {
        refs::resolve(&self.model, reference, &self.here)
    }

    pub fn named(&self, reference: &str) -> Named {
        plan::named(&self.model, reference, &self.here)
    }

    /// The plan for one node of a node type, named after it.
    pub fn plan_node(&self, reference: &str) -> Result<Plan, Diag> {
        plan::single(&self.model, reference, &self.here)
    }

    pub fn group(&self, reference: &str) -> Result<GroupInfo, Diag> {
        let groups = Groups::new(&self.model);
        let (def, params) = self.group_def(&groups, reference)?;
        Ok(GroupInfo { includes: groups.includes(&def)?, file: def.file.clone(), description: def.description.clone(), params })
    }

    /// The plan for a group started with `key=value` words, each read as its param's type.
    pub fn plan_group(&self, reference: &str, words: &[String]) -> Result<GroupPlan, Diag> {
        let groups = Groups::new(&self.model);
        let (def, given) = self.group_given(&groups, reference, words)?;
        let (plan, values) = groups.plan(&def, given.into_iter().collect())?;
        Ok(GroupPlan { group: def.file.name.clone(), plan, values })
    }

    pub fn group_names(&self) -> Vec<GroupName<'_>> {
        let groups = Groups::new(&self.model);
        let here = self.package_here();
        self.model.groups.iter().map(|file| GroupName { file, short: groups.short_name(file, here) }).collect()
    }

    pub fn services(&self) -> Vec<ServiceCheck<'_>> {
        let groups = Groups::new(&self.model);
        let check = |service| ServiceCheck { service, checked: service.check(&self.model, &groups) };
        self.model.settings.services.iter().map(check).collect()
    }

    /// Adds a service block for a group started with `key=value` words to the file that lists
    /// the services. False when one with the same params is there already.
    pub fn add_service(&self, reference: &str, words: &[String]) -> Result<bool, Diag> {
        let groups = Groups::new(&self.model);
        let (def, given) = self.group_given(&groups, reference, words)?;
        service::add(&self.model, &groups, &def, given)
    }

    /// Removes the service blocks at these places among `services`, keeping the rest as written.
    pub fn remove_services(&self, indexes: &[usize]) -> Result<(), Diag> {
        service::remove(&self.model.settings.services_file, indexes)
    }

    /// The build plan for these packages and all they depend on, every package for none.
    pub fn build_plan(&self, wanted: &[String]) -> Result<BuildPlan, Diag> {
        build_plan::plan(&self.model, wanted)
    }

    /// The package holding here, where a bare group name is looked for first.
    fn package_here(&self) -> Option<&str> {
        self.model.package_holding(&self.here).map(|p| p.name.as_str())
    }

    fn group_def(&self, groups: &Groups, reference: &str) -> Result<(Rc<GroupDef>, Vec<Param>), Diag> {
        let file = groups.find(reference, self.package_here()).map_err(Diag::plain)?;
        let def = groups.load(file)?;
        let params = groups.interface(&def)?;
        Ok((def, params))
    }

    /// A group and the values `key=value` words give its params, a path counted from here.
    fn group_given(&self, groups: &Groups, reference: &str, words: &[String]) -> Result<(Rc<GroupDef>, Given), Diag> {
        let (def, params) = self.group_def(groups, reference)?;
        let given = param::from_words(&params, &def.file.name, words, &self.here)?;
        Ok((def, given))
    }
}
