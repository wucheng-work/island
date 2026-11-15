//! Open-Meteo 天气查询。只在页面打开且缓存过期时启动短时工作线程。

use super::http::{Client, Method};
use crate::events::{AppEvent, EventSink};
use crate::state::{WeatherCity, WeatherSnapshot};
use poni_core::urlenc::encode;
use serde_json::Value;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(8);

pub fn search_cities(query: String, search_id: u64, sink: EventSink) {
    std::thread::spawn(move || {
        let result = find_cities(&query);
        sink.send(AppEvent::WeatherCityResults { search_id, result });
    });
}

pub fn fetch_city(city: String, request_id: u64, sink: EventSink) {
    std::thread::spawn(move || {
        let result = (|| {
            // 兼容尚未保存坐标的旧城市设置；候选选择后的设置直接使用坐标。
            let name = city.split(" · ").next().unwrap_or(&city);
            let places = find_cities(name)?;
            let place = places
                .into_iter()
                .find(|place| {
                    let label = place.label();
                    city.split(" · ")
                        .skip(1)
                        .all(|region| label.split(" · ").any(|part| part == region))
                })
                .ok_or("找不到这个城市，请重新选择地点")?;
            fetch(place.latitude, place.longitude, place.label())
        })();
        sink.send(AppEvent::WeatherForecast { request_id, result });
    });
}

/// 地名索引分别记录城市简称和正式名称，缺少市级命中时补查另一种写法。
fn find_cities(query: &str) -> Result<Vec<WeatherCity>, String> {
    let mut places = search_places(query)?;
    let chinese = query
        .chars()
        .all(|c| ('\u{3400}'..='\u{9fff}').contains(&c));
    if chinese
        && !places.iter().any(|place| {
            matches!(
                place["feature_code"].as_str(),
                Some("PPLC" | "PPLA" | "PPLA2")
            )
        })
    {
        // 仅在用户搜索且未命中市级地点时补查简称/全称，不增加后台轮询。
        if let Some(short) = query.strip_suffix('市') {
            places.extend(search_places(short)?);
        } else if !query.ends_with(['省', '县', '区', '镇', '乡', '村', '旗'])
            && !query.ends_with("自治州")
        {
            places.extend(search_places(&format!("{query}市"))?);
        }
    }
    places.sort_by(|a, b| {
        let rank = |place: &Value| match place["feature_code"].as_str().unwrap_or("") {
            "PPLC" => 0,
            "PPLA" => 1,
            "PPLA2" => 2,
            "PPLA3" => 3,
            "PPLA4" => 4,
            _ => 5,
        };
        rank(a).cmp(&rank(b)).then_with(|| {
            b["population"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["population"].as_u64().unwrap_or(0))
        })
    });
    let mut ids = std::collections::HashSet::new();
    let mut cities = Vec::new();
    for place in places {
        if let Some(id) = place["id"].as_u64() {
            if !ids.insert(id) {
                continue;
            }
        }
        let (Some(name), Some(latitude), Some(longitude)) = (
            place["name"].as_str(),
            place["latitude"].as_f64(),
            place["longitude"].as_f64(),
        ) else {
            continue;
        };
        let mut regions = Vec::new();
        for key in ["admin1", "admin2", "admin3"] {
            if let Some(region) = place[key]
                .as_str()
                .filter(|region| !region.is_empty() && *region != name)
            {
                if !regions.contains(&region) {
                    regions.push(region);
                }
            }
        }
        cities.push(WeatherCity {
            name: name.to_string(),
            region: regions.join(" · "),
            country: place["country"].as_str().unwrap_or("").to_string(),
            latitude,
            longitude,
        });
        if cities.len() == 4 {
            break;
        }
    }
    if cities.is_empty() {
        return Err("找不到匹配的城市".to_string());
    }
    Ok(cities)
}

fn search_places(query: &str) -> Result<Vec<Value>, String> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=20&language=zh&format=json",
        encode(query)
    );
    let body = get(&url)?;
    Ok(body["results"].as_array().cloned().unwrap_or_default())
}

pub fn fetch_location(
    latitude: f64,
    longitude: f64,
    place: String,
    request_id: u64,
    sink: EventSink,
) {
    std::thread::spawn(move || {
        let result = fetch(latitude, longitude, place);
        sink.send(AppEvent::WeatherForecast { request_id, result });
    });
}

fn fetch(latitude: f64, longitude: f64, place: String) -> Result<WeatherSnapshot, String> {
    // 百米级坐标足够查询天气，避免向服务端发送设备的完整定位精度。
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude:.3}&longitude={longitude:.3}&current=temperature_2m,weather_code,is_day&timezone=auto"
    );
    let body = get(&url)?;
    let current = &body["current"];
    let temperature_c = current["temperature_2m"].as_f64().ok_or("天气数据不完整")? as f32;
    let code = current["weather_code"].as_u64().ok_or("天气数据不完整")? as u8;
    let is_day = current["is_day"].as_u64().ok_or("天气数据不完整")? == 1;
    Ok(WeatherSnapshot {
        place,
        temperature_c,
        code,
        is_day,
        updated_unix: crate::clock::unix_now(),
    })
}

fn get(url: &str) -> Result<Value, String> {
    // Windows 的短时天气线程也必须先初始化 COM，再创建 WinRT HTTP 对象。
    #[cfg(windows)]
    let _mta = super::http::MtaGuard::new();
    let client = Client::private(concat!("Island/", env!("CARGO_PKG_VERSION"), " weather"))
        .map_err(|error| {
            crate::pwarn!("weather: HTTP client unavailable ({error})");
            "天气网络不可用".to_string()
        })?;
    let response = client
        .send(Method::Get, url, &[], None, TIMEOUT)
        .map_err(|error| {
            crate::pwarn!("weather: request failed ({error})");
            "天气网络不可用".to_string()
        })?;
    if response.status != 200 {
        crate::pwarn!("weather: service answered HTTP {}", response.status);
        return Err("天气服务暂时不可用".to_string());
    }
    serde_json::from_str(&response.body).map_err(|_| "天气数据无法读取".to_string())
}
