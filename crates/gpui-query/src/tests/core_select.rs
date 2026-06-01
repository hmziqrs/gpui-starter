use crate::core::*;

#[test]
fn test_select_transform_apply() {
    let transform = SelectTransform::new(|data: &String| data.len());
    assert_eq!(transform.apply(&"hello".to_string()), 5);
    assert_eq!(transform.apply(&"".to_string()), 0);
    assert_eq!(transform.apply(&"world!".to_string()), 6);
}

#[test]
fn test_mapped_query_with_data() {
    let transform = SelectTransform::new(|data: &String| data.to_uppercase());
    let mapped = MappedQueryResource::<String, String, QueryError>::new(
        Some("hello".to_string()),
        transform,
    );

    assert_eq!(mapped.data(), Some("HELLO".to_string()));
    assert!(mapped.has_data());
}

#[test]
fn test_mapped_query_without_data() {
    let transform = SelectTransform::new(|data: &String| data.to_uppercase());
    let mapped = MappedQueryResource::<String, String, QueryError>::new(None, transform);

    assert_eq!(mapped.data(), None);
    assert!(!mapped.has_data());
}

#[test]
fn test_select_transform_clone() {
    let transform = SelectTransform::new(|data: &String| data.len());
    let cloned = transform.clone();

    assert_eq!(transform.apply(&"hello".to_string()), 5);
    assert_eq!(cloned.apply(&"hello".to_string()), 5);
    assert_eq!(transform.apply(&"world".to_string()), 5);
    assert_eq!(cloned.apply(&"world".to_string()), 5);
}

#[test]
fn test_mapped_query_with_numeric_transform() {
    // Test with different input/output types
    let transform = SelectTransform::new(|data: &i32| *data as f64 * 1.5);
    let mapped = MappedQueryResource::<i32, f64, QueryError>::new(Some(10), transform);

    let result = mapped.data().expect("should have data");
    assert!((result - 15.0).abs() < f64::EPSILON);
}

#[test]
fn test_mapped_query_with_struct_transform() {
    #[derive(Clone, Debug)]
    struct User {
        name: String,
        age: u32,
    }

    let transform = SelectTransform::new(|user: &User| format!("{} ({})", user.name, user.age));
    let mapped = MappedQueryResource::<User, String, QueryError>::new(
        Some(User {
            name: "Alice".to_string(),
            age: 30,
        }),
        transform,
    );

    assert_eq!(mapped.data(), Some("Alice (30)".to_string()));
}
