// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

use rstd::prelude::*;

use codec::{Codec, Decode, Encode};
use rstd::collections::btree_set::BTreeSet;
use runtime_primitives::traits::{MaybeSerializeDebug, Member, SimpleArithmetic};
use srml_support::{decl_module, decl_storage, dispatch, ensure, Parameter, StorageMap};
use system;

// EntityId, ClassId -> should be configured on versioned_store::Trait
pub use versioned_store::{ClassId, ClassPropertyValue, EntityId, Property, PropertyValue};
pub type PropertyIndex = u16; // should really be configured on versioned_store::Trait

mod mock;
mod principles;
mod tests;

pub use principles::*;

/// Trait that provides an abstraction for the concept of group membership and a way
/// to check the inclusion of an account id in a specific group.
pub trait GroupMembershipChecker<T: Trait> {
    fn account_is_in_group(account: &T::AccountId, group: T::GroupId) -> bool;
}

/// An implementation where groups are effectively disabled. No account will ever
/// be reported to be a member of any group.
impl<T: Trait> GroupMembershipChecker<T> for () {
    fn account_is_in_group(_account: &T::AccountId, _group: T::GroupId) -> bool {
        false
    }
}

/// An implementation that calls into multiple checkers. This allows for multiple modules
/// to maintain different group membership information.
impl<T: Trait, X: GroupMembershipChecker<T>, Y: GroupMembershipChecker<T>> GroupMembershipChecker<T>
    for (X, Y)
{
    fn account_is_in_group(account: &T::AccountId, group: T::GroupId) -> bool {
        X::account_is_in_group(account, group) || Y::account_is_in_group(account, group)
    }
}

/// Trait for externally checking if an account can create new classes in the versioned store.
pub trait CreateClassPermissionsChecker<T: Trait> {
    fn account_can_create_class_permissions(account: &T::AccountId) -> bool;
}

/// An implementation that does not permit any account to create classes. Effectively leaving
/// only permission for the system.
impl<T: Trait> CreateClassPermissionsChecker<T> for () {
    fn account_can_create_class_permissions(_account: &T::AccountId) -> bool {
        false
    }
}

/// Internal type, derived from dispatchable call, identifies the caller
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, Clone, Debug)]
enum DerivedPrincipal<AccountId, GroupId> {
    /// ROOT origin
    System,
    /// Caller correctly identified as entity owner
    EntityOwner, // Maybe enclose EntityId?
    /// Plain signed origin, or additionally identified as beloging to specific group
    Base(BasePrincipal<AccountId, GroupId>),
}

/// Pointer to a specific property of entities of a specfic class.
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, Clone, Debug)]
pub struct PropertyOfClass<ClassId, PropertyIndex> {
    class_id: ClassId,
    property_index: PropertyIndex,
}

/// The type of constraint on what entities can reference instances of a class through an Internal property type.
#[derive(Encode, Decode, Eq, PartialEq, Clone, Debug)]
pub enum ReferenceConstraint<ClassId: Ord, PropertyIndex: Ord> {
    /// No Entity can reference the class.
    NoReferencingAllowed,

    /// Any entity may reference the class.
    NoConstraint,

    /// Only a set of entities of type ClassId and from the specified property index can reference the class.
    Restricted(BTreeSet<PropertyOfClass<ClassId, PropertyIndex>>),
}

impl<ClassId: Ord, PropertyIndex: Ord> Default for ReferenceConstraint<ClassId, PropertyIndex> {
    fn default() -> Self {
        ReferenceConstraint::NoReferencingAllowed
    }
}

/// Permissions for an instance of a Class in the versioned store.
#[derive(Encode, Decode, Default, Eq, PartialEq, Clone, Debug)]
pub struct ClassPermissions<ClassId, AccountId, GroupId, PropertyIndex, BlockNumber>
where
    ClassId: Ord,
    AccountId: Ord + Clone,
    GroupId: Ord + Clone,
    PropertyIndex: Ord,
{
    // concrete permissions
    /// Permissions that are applied to entities of this class, defines who in addition to
    /// root origin can update and delete entities of this class.
    entity_permissions: EntityPermissions<AccountId, GroupId>,

    /// Wether new entities of this class be created or not. Is not enforced for root origin.
    entities_can_be_created: bool,

    /// Who can add new schemas in the versioned store for this class
    add_schemas: BasePrincipalSet<AccountId, GroupId>,

    /// Who can create new entities in the versioned store of this class
    create_entities: BasePrincipalSet<AccountId, GroupId>,

    /// The type of constraint on referencing the class from other entities.
    reference_constraint: ReferenceConstraint<ClassId, PropertyIndex>,

    /// Who (in addition to root origin) can update all concrete permissions.
    /// The admins can only be set by the root origin, "System".
    admins: BasePrincipalSet<AccountId, GroupId>,

    // Block where permissions were changed
    last_permissions_update: BlockNumber,
}

pub type ClassPermissionsType<T> = ClassPermissions<
    ClassId,
    <T as system::Trait>::AccountId,
    <T as Trait>::GroupId,
    PropertyIndex,
    <T as system::Trait>::BlockNumber,
>;

impl<ClassId, AccountId, GroupId, PropertyIndex, BlockNumber>
    ClassPermissions<ClassId, AccountId, GroupId, PropertyIndex, BlockNumber>
where
    ClassId: Ord,
    AccountId: Ord + Clone,
    GroupId: Ord + Clone,
    PropertyIndex: Ord,
{
    /// Returns Ok if principal is root origin or base_principal is in admins set, Err otherwise
    fn is_admin(
        class_permissions: &Self,
        derived_principal: &DerivedPrincipal<AccountId, GroupId>,
    ) -> dispatch::Result {
        match derived_principal {
            DerivedPrincipal::System => Ok(()),
            DerivedPrincipal::Base(base_principal) => {
                if class_permissions.admins.contains(base_principal) {
                    Ok(())
                } else {
                    Err("NotInAdminsSet")
                }
            }
            _ => Err("DerivedPrincipal::EntityOwner-UsedOutOfPlace"),
        }
    }

    fn can_add_schema(
        class_permissions: &Self,
        derived_principal: &DerivedPrincipal<AccountId, GroupId>,
    ) -> dispatch::Result {
        match derived_principal {
            DerivedPrincipal::System => Ok(()),
            DerivedPrincipal::Base(base_principal) => {
                if class_permissions.add_schemas.contains(base_principal) {
                    Ok(())
                } else {
                    Err("NotInAddSchemasSet")
                }
            }
            _ => Err("DerivedPrincipal::EntityOwner-UsedOutOfPlace"),
        }
    }

    fn can_create_entity(
        class_permissions: &Self,
        derived_principal: &DerivedPrincipal<AccountId, GroupId>,
    ) -> dispatch::Result {
        match derived_principal {
            DerivedPrincipal::System => Ok(()),
            DerivedPrincipal::Base(base_principal) => {
                if !class_permissions.entities_can_be_created {
                    Err("EntitiesCannotBeCreated")
                } else if class_permissions.create_entities.contains(base_principal) {
                    Ok(())
                } else {
                    Err("NotInCreateEntitiesSet")
                }
            }
            _ => Err("DerivedPrincipal::EntityOwner-UsedOutOfPlace"),
        }
    }

    fn can_update_entity(
        class_permissions: &Self,
        derived_principal: &DerivedPrincipal<AccountId, GroupId>,
    ) -> dispatch::Result {
        match derived_principal {
            DerivedPrincipal::System => Ok(()),
            DerivedPrincipal::Base(base_principal) => {
                if class_permissions
                    .entity_permissions
                    .update
                    .contains(&EntityPrincipal::Base(base_principal.clone()))
                {
                    Ok(())
                } else {
                    Err("NotInEntityPermissionsUpdateSet")
                }
            }
            DerivedPrincipal::EntityOwner => {
                if class_permissions
                    .entity_permissions
                    .update
                    .contains(&EntityPrincipal::Owner)
                {
                    Ok(())
                } else {
                    Err("NotInEntityPermissionsUpdateSet")
                }
            }
        }
    }

    fn can_transfer_entity_ownership(
        class_permissions: &Self,
        derived_principal: &DerivedPrincipal<AccountId, GroupId>,
    ) -> dispatch::Result {
        match derived_principal {
            DerivedPrincipal::System => Ok(()),
            DerivedPrincipal::Base(base_principal) => {
                if class_permissions
                    .entity_permissions
                    .transfer_ownership
                    .contains(&EntityPrincipal::Base(base_principal.clone()))
                {
                    Ok(())
                } else {
                    Err("NotInEntityPermissionsTransferOwnershipSet")
                }
            }
            DerivedPrincipal::EntityOwner => {
                if class_permissions
                    .entity_permissions
                    .transfer_ownership
                    .contains(&EntityPrincipal::Owner)
                {
                    Ok(())
                } else {
                    Err("NotInEntityPermissionsTransferOwnershipSet")
                }
            }
        }
    }
}

#[derive(Encode, Decode, Clone, Debug, Default, Eq, PartialEq)]
pub struct EntityPermissions<AccountId, GroupId>
where
    AccountId: Ord,
    GroupId: Ord,
{
    update: EntityPrincipalSet<AccountId, GroupId>,
    delete: EntityPrincipalSet<AccountId, GroupId>,
    transfer_ownership: EntityPrincipalSet<AccountId, GroupId>,
}

pub trait Trait: system::Trait + versioned_store::Trait {
    // type Event: ...
    // Do we need Events?

    type GroupId: Parameter
        + Member
        + SimpleArithmetic
        + Codec
        + Default
        + Copy
        + Clone
        + MaybeSerializeDebug
        + Eq
        + PartialEq
        + Ord;

    /// External type used to check if an account is a member of a specific group.
    type GroupMembershipChecker: GroupMembershipChecker<Self>;

    /// External type used to check if an account id has permission to create new class permissions.
    type CreateClassPermissionsChecker: CreateClassPermissionsChecker<Self>;
}

decl_storage! {
    trait Store for Module<T: Trait> as VersionedStorePermissions {
      /// ClassPermissions of corresponding Classes in the versioned store
      pub ClassPermissionsByClassId get(class_permissions_by_class_id): linked_map ClassId => ClassPermissionsType<T>;

      /// Owner of an entity in the versioned store. If it is None then it is owned by the system.
      pub EntityOwnerByEntityId get(entity_owner_by_entity_id): linked_map EntityId => Option<BasePrincipal<T::AccountId, T::GroupId>>;
    }
}

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {

        /// Sets the admins for a class
        fn set_class_admins(
            origin,
            class_id: ClassId,
            admins: BasePrincipalSet<T::AccountId, T::GroupId>
        ) -> dispatch::Result {
            Self::mutate_class_permissions(
                origin,
                None,
                Self::is_system_principal, // root origin
                class_id,
                |class_permissions| {
                    class_permissions.admins = admins;
                    Ok(())
                }
            )
        }

        // Methods for updating concrete permissions

        fn set_class_entity_permissions(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId,
            entity_permissions: EntityPermissions<T::AccountId, T::GroupId>
        ) -> dispatch::Result {
            Self::mutate_class_permissions(
                origin,
                claimed_group_id,
                ClassPermissions::is_admin,
                class_id,
                |class_permissions| {
                    class_permissions.entity_permissions = entity_permissions;
                    Ok(())
                }
            )
        }

        fn set_class_entities_can_be_created(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId,
            can_be_created: bool
        ) -> dispatch::Result {
            Self::mutate_class_permissions(
                origin,
                claimed_group_id,
                ClassPermissions::is_admin,
                class_id,
                |class_permissions| {
                    class_permissions.entities_can_be_created = can_be_created;
                    Ok(())
                }
            )
        }

        fn set_class_add_schemas_set(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId,
            base_principal_set: BasePrincipalSet<T::AccountId, T::GroupId>
        ) -> dispatch::Result {
            Self::mutate_class_permissions(
                origin,
                claimed_group_id,
                ClassPermissions::is_admin,
                class_id,
                |class_permissions| {
                    class_permissions.add_schemas = base_principal_set;
                    Ok(())
                }
            )
        }

        fn set_class_create_entities_set(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId,
            base_principal_set: BasePrincipalSet<T::AccountId, T::GroupId>
        ) -> dispatch::Result {
            Self::mutate_class_permissions(
                origin,
                claimed_group_id,
                ClassPermissions::is_admin,
                class_id,
                |class_permissions| {
                    class_permissions.create_entities = base_principal_set;
                    Ok(())
                }
            )
        }

        fn set_class_reference_constraint(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId,
            constraint: ReferenceConstraint<ClassId, PropertyIndex>
        ) -> dispatch::Result {
            Self::mutate_class_permissions(
                origin,
                claimed_group_id,
                ClassPermissions::is_admin,
                class_id,
                |class_permissions| {
                    class_permissions.reference_constraint = constraint;
                    Ok(())
                }
            )
        }

        pub fn set_entity_owner(
            origin,
            claimed_group_id: Option<T::GroupId>,
            as_entity_owner: bool,
            entity_id: EntityId,
            new_owner: Option<BasePrincipal<T::AccountId, T::GroupId>>
        ) -> dispatch::Result {
            let class_id = Self::get_class_id_by_entity_id(&entity_id)?;
            let as_entity_owner = if as_entity_owner {
                Some(entity_id)
            } else {
                None
            };
            Self::if_class_permissions_satisfied(
                origin,
                claimed_group_id,
                as_entity_owner,
                ClassPermissions::can_transfer_entity_ownership,
                class_id,
                |_class_permissions, _principal| {
                    if let Some(base_principal) = new_owner {
                        <EntityOwnerByEntityId<T>>::mutate(entity_id, |owner| {
                            *owner = Some(base_principal.clone());
                        });
                    } else {
                        // transfer ownership to System
                        <EntityOwnerByEntityId<T>>::remove(entity_id);
                    }
                    Ok(())
                }
            )
        }

        // Permissioned proxy calls to versioned store

        pub fn create_class(
            origin,
            name: Vec<u8>,
            description: Vec<u8>,
            class_permissions: ClassPermissionsType<T>
        ) -> dispatch::Result {

            let can_create_class = match origin.into() {
                Ok(system::RawOrigin::Root) => true,
                Ok(system::RawOrigin::Signed(sender)) => {
                    T::CreateClassPermissionsChecker::account_can_create_class_permissions(&sender)
                },
                _ => false
            };

            if can_create_class {
                let class_id = <versioned_store::Module<T>>::create_class(name, description)?;

                // is there a need to assert class_id is unique?

                <ClassPermissionsByClassId<T>>::insert(&class_id, class_permissions);

                Ok(())
            } else {
                Err("NotPermittedToCreateClass")
            }
        }

        pub fn create_class_with_default_permissions(
            origin,
            name: Vec<u8>,
            description: Vec<u8>
        ) -> dispatch::Result {
            Self::create_class(origin, name, description, ClassPermissions::default())
        }

        pub fn add_class_schema(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId,
            existing_properties: Vec<PropertyIndex>,
            new_properties: Vec<Property>
        ) -> dispatch::Result {
            Self::if_class_permissions_satisfied(
                origin,
                claimed_group_id,
                None,
                ClassPermissions::can_add_schema,
                class_id,
                |_class_permissions, _principal| {
                    // If a new property points at another class,
                    // at this point we don't enforce anything about reference constraints
                    // because of the chicken and egg problem. Instead enforcement is done
                    // at the time of creating an entity.
                    let _schema_index = <versioned_store::Module<T>>::add_class_schema(class_id, existing_properties, new_properties)?;
                    Ok(())
                }
            )
        }

        pub fn create_entity(
            origin,
            claimed_group_id: Option<T::GroupId>,
            class_id: ClassId
            //, owner: BasePrincipal<T::AccountId, T::GroupId>
        ) -> dispatch::Result {
            Self::if_class_permissions_satisfied(
                origin,
                claimed_group_id,
                None,
                ClassPermissions::can_create_entity,
                class_id,
                |_class_permissions, principal| {
                    let entity_id = <versioned_store::Module<T>>::create_entity(class_id)?;

                    // Note: mutating value to None is equivalient to removing the value from storage map
                    <EntityOwnerByEntityId<T>>::mutate(entity_id, |owner| {
                        match principal {
                            DerivedPrincipal::System => *owner = None,
                            DerivedPrincipal::Base(base_principal) => *owner = Some(base_principal.clone()),
                            _ => *owner = None
                        }
                    });
                    Ok(())
                }
            )
        }

        pub fn add_schema_support_to_entity(
            origin,
            claimed_group_id: Option<T::GroupId>,
            as_entity_owner: bool,
            entity_id: EntityId,
            schema_id: u16,
            property_values: Vec<ClassPropertyValue>
        ) -> dispatch::Result {
            // class id of the entity being updated
            let class_id = Self::get_class_id_by_entity_id(&entity_id)?;

            Self::ensure_internal_property_values_permitted(class_id, &property_values)?;

            let as_entity_owner = if as_entity_owner {
                Some(entity_id)
            } else {
                None
            };

            Self::if_class_permissions_satisfied(
                origin,
                claimed_group_id,
                as_entity_owner,
                ClassPermissions::can_update_entity,
                class_id,
                |_class_permissions, _principal| {
                    <versioned_store::Module<T>>::add_schema_support_to_entity(entity_id, schema_id, property_values)
                }
            )
        }

        pub fn update_entity_property_values(
            origin,
            claimed_group_id: Option<T::GroupId>,
            as_entity_owner: bool,
            entity_id: EntityId,
            property_values: Vec<ClassPropertyValue>
        ) -> dispatch::Result {
            let class_id = Self::get_class_id_by_entity_id(&entity_id)?;

            Self::ensure_internal_property_values_permitted(class_id, &property_values)?;

            let as_entity_owner = if as_entity_owner {
                Some(entity_id)
            } else {
                None
            };

            Self::if_class_permissions_satisfied(
                origin,
                claimed_group_id,
                as_entity_owner,
                ClassPermissions::can_update_entity,
                class_id,
                |_class_permissions, _principal| {
                    <versioned_store::Module<T>>::update_entity_property_values(entity_id, property_values)
                }
            )
        }
    }
}

impl<T: Trait> Module<T> {
    /// Attempts to derive the principal a caller is claiming.
    /// It expects only signed or root origin.
    fn derive_principal(
        origin: T::Origin,
        claimed_group_id: Option<T::GroupId>,
        as_entity_owner: Option<EntityId>,
    ) -> Result<DerivedPrincipal<T::AccountId, T::GroupId>, &'static str> {
        match origin.into() {
            Ok(system::RawOrigin::Root) => Ok(DerivedPrincipal::System),
            Ok(system::RawOrigin::Signed(account_id)) => {
                if let Some(group_id) = claimed_group_id {
                    if T::GroupMembershipChecker::account_is_in_group(&account_id, group_id) {
                        if let Some(entity_id) = as_entity_owner {
                            // is entity owned by system
                            ensure!(
                                <EntityOwnerByEntityId<T>>::exists(entity_id),
                                "NotEnityOwner"
                            );
                            // ensure entity owner is GroupMember
                            match Self::entity_owner_by_entity_id(entity_id) {
                                Some(BasePrincipal::GroupMember(owner_group_id))
                                    if group_id == owner_group_id =>
                                {
                                    Ok(DerivedPrincipal::EntityOwner)
                                }
                                _ => Err("NotEnityOwner"),
                            }
                        } else {
                            Ok(DerivedPrincipal::Base(BasePrincipal::GroupMember(group_id)))
                        }
                    } else {
                        Err("OriginNotMemberOfClaimedGroup")
                    }
                } else {
                    if let Some(entity_id) = as_entity_owner {
                        // is entity owned by system?
                        ensure!(
                            <EntityOwnerByEntityId<T>>::exists(entity_id),
                            "NotEnityOwner"
                        );
                        // ensure entity owner is Account
                        match Self::entity_owner_by_entity_id(entity_id) {
                            Some(BasePrincipal::Account(ref owner_account_id))
                                if account_id == *owner_account_id =>
                            {
                                Ok(DerivedPrincipal::EntityOwner)
                            }
                            _ => Err("NotEnityOwner"),
                        }
                    } else {
                        Ok(DerivedPrincipal::Base(BasePrincipal::Account(account_id)))
                    }
                }
            }
            _ => Err("BadOrigin:ExpectedRootOrSigned"),
        }
    }

    /// Returns the stored class permissions if exist, error otherwise.
    fn ensure_class_permissions(
        class_id: ClassId,
    ) -> Result<ClassPermissionsType<T>, &'static str> {
        ensure!(
            <ClassPermissionsByClassId<T>>::exists(class_id),
            "ClassIdDoesNotExist"
        );
        Ok(Self::class_permissions_by_class_id(class_id))
    }

    /// Constructs a derived principal from the origin and claimed group id.
    /// If the predicate passes, the mutate method is invoked.
    fn mutate_class_permissions<Predicate, Mutate>(
        origin: T::Origin,
        claimed_group_id: Option<T::GroupId>,
        // predicate to test
        predicate: Predicate,
        // class permissions to perform mutation on if it exists
        class_id: ClassId,
        // actual mutation to apply.
        mutate: Mutate,
    ) -> dispatch::Result
    where
        Predicate: FnOnce(
            &ClassPermissionsType<T>,
            &DerivedPrincipal<T::AccountId, T::GroupId>,
        ) -> dispatch::Result,
        Mutate: FnOnce(&mut ClassPermissionsType<T>) -> dispatch::Result,
    {
        let principal = Self::derive_principal(origin, claimed_group_id, None)?;
        let mut class_permissions = Self::ensure_class_permissions(class_id)?;

        predicate(&class_permissions, &principal)?;
        mutate(&mut class_permissions)?;
        class_permissions.last_permissions_update = <system::Module<T>>::block_number();
        <ClassPermissionsByClassId<T>>::insert(class_id, class_permissions);
        Ok(())
    }

    fn is_system_principal(
        _: &ClassPermissionsType<T>,
        principal: &DerivedPrincipal<T::AccountId, T::GroupId>,
    ) -> dispatch::Result {
        if *principal == DerivedPrincipal::System {
            Ok(())
        } else {
            Err("NotRootOrigin")
        }
    }

    /// Constructs a derived principal from the origin and claimed group id.
    /// If the peridcate passes the callback is invoked. Returns result of the callback
    /// or error from failed predicate.
    fn if_class_permissions_satisfied<Predicate, Callback>(
        origin: T::Origin,
        claimed_group_id: Option<T::GroupId>,
        as_entity_owner: Option<EntityId>,
        // predicate to test
        predicate: Predicate,
        // class permissions to test
        class_id: ClassId,
        // callback to invoke if predicate passes
        callback: Callback,
    ) -> dispatch::Result
    where
        Predicate: FnOnce(
            &ClassPermissionsType<T>,
            &DerivedPrincipal<T::AccountId, T::GroupId>,
        ) -> dispatch::Result,
        Callback: FnOnce(
            &ClassPermissionsType<T>,
            &DerivedPrincipal<T::AccountId, T::GroupId>,
        ) -> dispatch::Result,
    {
        let principal = Self::derive_principal(origin, claimed_group_id, as_entity_owner)?;
        let class_permissions = Self::ensure_class_permissions(class_id)?;

        predicate(&class_permissions, &principal)?;
        callback(&class_permissions, &principal)
    }

    fn get_class_id_by_entity_id(entity_id: &EntityId) -> Result<ClassId, &'static str> {
        // use a utility method on versioned_store module
        ensure!(
            versioned_store::EntityById::exists(entity_id),
            "EntityNotFound"
        );
        let entity = <versioned_store::Module<T>>::entity_by_id(entity_id);
        Ok(entity.class_id)
    }

    // Ensures property_values of type Internal that point to a class,
    // the target entity and class exists and constraint allows it.
    fn ensure_internal_property_values_permitted(
        source_class_id: ClassId,
        property_values: &Vec<ClassPropertyValue>,
    ) -> dispatch::Result {
        for property_value in property_values.iter() {
            if let PropertyValue::Internal(ref target_entity_id) = property_value.value {
                // get the class permissions for target class
                let target_class_id = Self::get_class_id_by_entity_id(target_entity_id)?;
                // assert class permissions exists for target class
                let class_permissions = Self::class_permissions_by_class_id(target_class_id);

                // ensure internal reference is permitted
                match class_permissions.reference_constraint {
                    ReferenceConstraint::NoConstraint => Ok(()),
                    ReferenceConstraint::NoReferencingAllowed => {
                        Err("EntityCannotReferenceTargetEntity")
                    }
                    ReferenceConstraint::Restricted(permitted_properties) => {
                        if permitted_properties.contains(&PropertyOfClass {
                            class_id: source_class_id,
                            property_index: property_value.in_class_index,
                        }) {
                            Ok(())
                        } else {
                            Err("EntityCannotReferenceTargetEntity")
                        }
                    }
                }?;
            }
        }

        // if we reach here all Internal properties have passed the constraint check
        Ok(())
    }
}
