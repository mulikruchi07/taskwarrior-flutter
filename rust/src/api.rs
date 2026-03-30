use flutter_rust_bridge::frb;
use taskchampion::{
    chrono::{DateTime, Utc},
    Operations, Replica, ServerConfig, StorageConfig, Tag,
};
use uuid::Uuid;
use std::{collections::HashMap, path::PathBuf, str::FromStr};
use serde_json;

fn parse_datetime(input: &str) -> Option<DateTime<Utc>> {
    if input.trim().is_empty() {
        return None;
    }
    input.parse::<DateTime<Utc>>().ok()
}

fn parse_tag_query(tags_query: &str) -> (Vec<String>, Vec<String>) {
    // Supports: "+work -home urgent" => include: ["work","urgent"], exclude:["home"]
    let mut include = vec![];
    let mut exclude = vec![];

    for raw in tags_query.split_whitespace() {
        if raw.is_empty() {
            continue;
        }

        if let Some(tag) = raw.strip_prefix('+') {
            if !tag.is_empty() {
                include.push(tag.to_string());
            }
        } else if let Some(tag) = raw.strip_prefix('-') {
            if !tag.is_empty() {
                exclude.push(tag.to_string());
            }
        } else {
            include.push(raw.to_string());
        }
    }

    (include, exclude)
}

fn task_matches_query(task: &HashMap<String, String>, query: &HashMap<String, String>) -> bool {
    // uuid exact
    if let Some(q_uuid) = query.get("uuid") {
        let q_uuid = q_uuid.trim();
        if !q_uuid.is_empty() {
            if task.get("uuid").map(|s| s.as_str()) != Some(q_uuid) {
                return false;
            }
        }
    }

    // status exact
    if let Some(q_status) = query.get("status") {
        let q_status = q_status.trim();
        if !q_status.is_empty() {
            if task.get("status").map(|s| s.as_str()) != Some(q_status) {
                return false;
            }
        }
    }

    // project exact
    if let Some(q_project) = query.get("project") {
        let q_project = q_project.trim();
        if !q_project.is_empty() {
            if task.get("project").map(|s| s.as_str()) != Some(q_project) {
                return false;
            }
        }
    }

    // tags with +include -exclude
    if let Some(q_tags) = query.get("tags") {
        let q_tags = q_tags.trim();
        if !q_tags.is_empty() {
            let tags_str = task.get("tags").map(|s| s.as_str()).unwrap_or("");

            let task_tags: std::collections::HashSet<&str> =
                tags_str.split_whitespace().filter(|t| !t.is_empty()).collect();

            let (include, exclude) = parse_tag_query(q_tags);

            for t in include {
                if !task_tags.contains(t.as_str()) {
                    return false;
                }
            }

            for t in exclude {
                if task_tags.contains(t.as_str()) {
                    return false;
                }
            }
        }
    }

    true
}

#[frb]
pub fn query_task(
    taskdb_dir_path: String,
    query: HashMap<String, String>,
) -> Result<String, taskchampion::Error> {
    let tasks = get_all_tasks(taskdb_dir_path);

    let filtered: Vec<HashMap<String, String>> = tasks
        .into_iter()
        .filter(|t| task_matches_query(t, &query))
        .collect();

    let json = serde_json::to_string(&filtered)
        .map_err(|e| taskchampion::Error::Other(anyhow::anyhow!(e)))?;
    Ok(json)
}

#[frb]
pub fn get_all_tasks_json(taskdb_dir_path: String) -> Result<String, taskchampion::Error> {
    let tasks = get_all_tasks(taskdb_dir_path); // your Vec<HashMap<String, String>>
    let json = serde_json::to_string(&tasks)
        .map_err(|e| taskchampion::Error::Other(anyhow::anyhow!(e)))?;
    Ok(json)
}

fn get_all_tasks(taskdb_dir_path: String) -> Vec<HashMap<String, String>> {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut vector: Vec<HashMap<String, String>> = Vec::new();

    for (_, value) in replica.all_tasks().unwrap() {
        let mut map: HashMap<String, String> = HashMap::new();
        let mut tags = "".to_string();

        for (k, v) in value.get_taskmap() {
            if k.contains("tag_") {
                if let Some(stripped) = k.strip_prefix("tag_") {
					tags.push_str(stripped);
					tags.push(' ');
                }
            } else {
                map.insert(k.into(), v.into());
            }
        }
        map.insert("tags".into(), tags.trim().into());
        map.insert("uuid".into(), value.get_uuid().to_string());
        vector.push(map);
    }
    vector
}

#[frb]
pub fn delete_task(uuid_st: String, taskdb_dir_path: String) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut ops = Operations::new();
    let uuid = Uuid::parse_str(&uuid_st).unwrap();

    if let Some(mut t) = replica.get_task_data(uuid).unwrap() {
        t.delete(&mut ops);
    }
    replica.commit_operations(ops).unwrap();
    0
}

#[frb]
pub fn update_task(
    uuid_st: String,
    taskdb_dir_path: String,
    map: HashMap<String, String>,
) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut ops = Operations::new();
    let uuid = Uuid::parse_str(&uuid_st).unwrap();

    if let Some(mut t) = replica.get_task(uuid).unwrap() {
        let _ = t.set_status(taskchampion::Status::Pending, &mut ops);
        for (key, value) in map {
            match key.as_str() {
                "description" => {
                    let _ = t.set_description(value, &mut ops);
                }
                "due" => {
                    let _ = t.set_due(parse_datetime(&value), &mut ops);
                }
                "start" => {
                    if value == "stop" {
                        let _ = t.stop(&mut ops);
                    } else {
                        let _ = t.start(&mut ops);
                    }
                }
                "wait" => {
                    let _ = t.set_wait(parse_datetime(&value), &mut ops);
                }
                "priority" => {
                    let _ = t.set_priority(value, &mut ops);
                }
                "tags" => {
					let existing_tags: Vec<String> = t
						.get_taskmap()
						.iter()
						.filter_map(|(k, _)| k.strip_prefix("tag_").map(|s| s.to_string()))
						.collect();
					for tag_name in existing_tags {
						println!("removing tag at rust side {}", tag_name);
						let mut tag = Tag::from_str(&tag_name).unwrap();
						let _ = t.remove_tag(&mut tag, &mut ops);
					}
                    
					for part in value.split_whitespace() {
                        println!("tag at rust side {}", part);
                        let mut tag = Tag::from_str(part).unwrap();
                        let _ = t.add_tag(&mut tag, &mut ops);
                    }
                }
                "project" => {
                    let _ = t.set_value("project", Some(value), &mut ops);
                }
                "status" => {
                    let status = match value.as_str() {
                        "pending" => taskchampion::Status::Pending,
                        "completed" => taskchampion::Status::Completed,
                        "deleted" => taskchampion::Status::Deleted,
                        _ => taskchampion::Status::Pending,
                    };
                    // print!("status at rust side {}", value);
                    println!("status at rust side {}", value);
                    let _ = t.set_status(status, &mut ops);
                }
                _ => {}
            }
        }
        replica.commit_operations(ops).unwrap();
    }
    0
}

#[frb]
pub fn add_task(taskdb_dir_path: String, map: HashMap<String, String>) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut ops = Operations::new();
    if let Some(uuid_str) = map.get("uuid") {
    let uuid = Uuid::parse_str(&uuid_str).unwrap();
    let mut t = replica.create_task(uuid, &mut ops).unwrap();

    let _ = t.set_status(taskchampion::Status::Pending, &mut ops);

    for (key, value) in map {
        match key.as_str() {
            "description" => {
                let _ = t.set_description(value, &mut ops);
            }
            "due" => {
                let _ = t.set_due(parse_datetime(&value), &mut ops);
            }
            "start" => {
                let _ = t.start(&mut ops);
            }
            "wait" => {
                let _ = t.set_wait(parse_datetime(&value), &mut ops);
            }
            "priority" => {
                let _ = t.set_priority(value, &mut ops);
            }
            "tags" => {
                for part in value.split_whitespace() {
                    let mut tag = Tag::from_str(part).unwrap();
                    let _ = t.add_tag(&mut tag, &mut ops);
                }
            }
            "project" => {
                let _ = t.set_user_defined_attribute("project", value, &mut ops);
            }
            _ => {}
        }
    }
    replica.commit_operations(ops).unwrap();
    return 0;
} 
    1
}

#[frb]
pub async fn sync(
    taskdb_dir_path: String,
    url: String,
    client_id: String,
    encryption_secret: String,
) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let config = ServerConfig::Remote {
        url: url.into(),
        client_id: Uuid::parse_str(&client_id).unwrap(),
        encryption_secret: encryption_secret.into(),
    };

    let mut server = config.into_server().unwrap();
    replica.sync(&mut server, false).unwrap();
    0
}

#[test]
fn test_add_task_with_tags() {
    use std::{collections::HashMap, env, fs};
    // create unique temporary directory for taskdb
    let tmp = env::temp_dir().join(format!("taskdb_test_{}", Uuid::new_v4()));
    let taskdb_path = tmp.to_string_lossy().into_owned();
    fs::create_dir_all(&tmp).expect("create temp taskdb dir");

    // prepare task map with tags
    let mut map: HashMap<String, String> = HashMap::new();
    let uuid = Uuid::new_v4().to_string();
    map.insert("uuid".to_string(), uuid.clone());
    map.insert("description".to_string(), "test task".to_string());
    map.insert("tags".to_string(), "tag1 tag2".to_string());

    // add task
    let res = add_task(taskdb_path.clone(), map);
    assert_eq!(res, 0);

    // read tasks as json and verify tags are present
    let json = get_all_tasks_json(taskdb_path.clone()).expect("get_all_tasks_json");
    let tasks: Vec<HashMap<String, String>> = serde_json::from_str(&json).expect("parse json");
    let found = tasks.into_iter().find(|m| m.get("uuid").map(|s| s == &uuid).unwrap_or(false));
    assert!(found.is_some(), "task with uuid not found");
    let task = found.unwrap();
    let tags = task.get("tags").map(|s| s.as_str()).unwrap_or("");
    assert!(tags.contains("tag1"), "tag1 missing in tags: {}", tags);
    assert!(tags.contains("tag2"), "tag2 missing in tags: {}", tags);

    // cleanup
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_query_task_filters_by_uuid_project_and_tags() {
    use std::{collections::HashMap, env, fs};

    let tmp = env::temp_dir().join(format!("taskdb_test_{}", Uuid::new_v4()));
    let taskdb_path = tmp.to_string_lossy().into_owned();
    fs::create_dir_all(&tmp).expect("create temp taskdb dir");

    let uuid1 = Uuid::new_v4().to_string();
    let uuid2 = Uuid::new_v4().to_string();

    let mut t1: HashMap<String, String> = HashMap::new();
    t1.insert("uuid".into(), uuid1.clone());
    t1.insert("description".into(), "task one".into());
    t1.insert("project".into(), "projA".into());
    t1.insert("tags".into(), "work urgent".into());
    assert_eq!(add_task(taskdb_path.clone(), t1), 0);

    let mut t2: HashMap<String, String> = HashMap::new();
    t2.insert("uuid".into(), uuid2.clone());
    t2.insert("description".into(), "task two".into());
    t2.insert("project".into(), "projB".into());
    t2.insert("tags".into(), "home".into());
    assert_eq!(add_task(taskdb_path.clone(), t2), 0);

    // uuid
    let mut q1: HashMap<String, String> = HashMap::new();
    q1.insert("uuid".into(), uuid1.clone());
    let json1 = query_task(taskdb_path.clone(), q1).expect("query_task");
    let res1: Vec<HashMap<String, String>> = serde_json::from_str(&json1).unwrap();
    assert_eq!(res1.len(), 1);
    assert_eq!(res1[0].get("uuid").unwrap(), &uuid1);

    // project
    let mut q2: HashMap<String, String> = HashMap::new();
    q2.insert("project".into(), "projB".into());
    let json2 = query_task(taskdb_path.clone(), q2).expect("query_task");
    let res2: Vec<HashMap<String, String>> = serde_json::from_str(&json2).unwrap();
    assert_eq!(res2.len(), 1);
    assert_eq!(res2[0].get("uuid").unwrap(), &uuid2);

    // tags +/-
    let mut q3: HashMap<String, String> = HashMap::new();
    q3.insert("tags".into(), "+work -home".into());
    let json3 = query_task(taskdb_path.clone(), q3).expect("query_task");
    let res3: Vec<HashMap<String, String>> = serde_json::from_str(&json3).unwrap();
    assert_eq!(res3.len(), 1);
    assert_eq!(res3[0].get("uuid").unwrap(), &uuid1);

    fs::remove_dir_all(&tmp).ok();
}