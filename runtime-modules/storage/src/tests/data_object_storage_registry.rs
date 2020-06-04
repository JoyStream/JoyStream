#![cfg(test)]

use super::mock::*;

#[test]
fn initial_state() {
    with_default_mock_builder(|| {
        assert_eq!(
            TestDataObjectStorageRegistry::first_relationship_id(),
            TEST_FIRST_RELATIONSHIP_ID
        );
    });
}

#[test]
fn test_add_relationship() {
    with_default_mock_builder(|| {
        // The content needs to exist - in our mock, that's with the content ID TEST_MOCK_EXISTING_CID
        let res = TestDataObjectStorageRegistry::add_relationship(
            Origin::signed(TEST_MOCK_LIAISON),
            TEST_MOCK_EXISTING_CID,
        );
        assert!(res.is_ok());
    });
}

#[test]
fn test_fail_adding_relationship_with_bad_content() {
    with_default_mock_builder(|| {
        let res = TestDataObjectStorageRegistry::add_relationship(Origin::signed(1), 24);
        assert!(res.is_err());
    });
}

#[test]
fn test_toggle_ready() {
    with_default_mock_builder(|| {
        // Create a DOSR
        let res = TestDataObjectStorageRegistry::add_relationship(
            Origin::signed(TEST_MOCK_LIAISON),
            TEST_MOCK_EXISTING_CID,
        );
        assert!(res.is_ok());

        // Grab DOSR ID from event
        let dosr_id = match System::events().last().unwrap().event {
            MetaEvent::data_object_storage_registry(
                data_object_storage_registry::RawEvent::DataObjectStorageRelationshipAdded(
                    dosr_id,
                    _content_id,
                    _account_id,
                ),
            ) => dosr_id,
            _ => 0xdeadbeefu64, // invalid value, unlikely to match
        };
        assert_ne!(dosr_id, 0xdeadbeefu64);

        // Toggling from a different account should fail
        let res = TestDataObjectStorageRegistry::set_relationship_ready(Origin::signed(2), dosr_id);
        assert!(res.is_err());

        // Toggling with the wrong ID should fail.
        let res = TestDataObjectStorageRegistry::set_relationship_ready(
            Origin::signed(TEST_MOCK_LIAISON),
            dosr_id + 1,
        );
        assert!(res.is_err());

        // Toggling with the correct ID and origin should succeed
        let res = TestDataObjectStorageRegistry::set_relationship_ready(
            Origin::signed(TEST_MOCK_LIAISON),
            dosr_id,
        );
        assert!(res.is_ok());
        assert_eq!(
            System::events().last().unwrap().event,
            MetaEvent::data_object_storage_registry(
                data_object_storage_registry::RawEvent::DataObjectStorageRelationshipReadyUpdated(
                    dosr_id, true,
                )
            )
        );
    });
}
