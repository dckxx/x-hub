//! 在线服务：连通性探活、天气（Open-Meteo）、城市地理编码、IP 定位、名言金句（hitokoto）。
//!
//! 设计原则：
//! - 全部走后端 reqwest，前端零 fetch（保持「唯一通道 = Tauri invoke」约定）；
//! - 云端只提供公开数据（天气 / 名言 / 经纬度解析），用户数据不出本地；
//! - 所有函数失败即返回错误，由命令层决定静默降级还是透出提示。

use serde::Serialize;

/// 天气当前数据（Open-Meteo `current` 字段子集）
#[derive(Debug, Clone, Serialize)]
pub struct WeatherCurrent {
    pub temperature: f64,
    pub apparent_temperature: f64,
    pub relative_humidity: f64,
    pub wind_speed: f64,
    /// WMO weather code，前端映射为图标 + 中文
    pub weather_code: i64,
    pub city: String,
}

/// 名言金句
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub content: String,
    /// 出处（hitokoto 的 from 字段，可能为空）
    pub from: String,
}

/// 经纬度 + 展示名（地理编码 / IP 定位的统一结果）
#[derive(Debug, Clone, Serialize)]
pub struct GeoLocation {
    pub name: String,
    pub lat: f64,
    pub lng: f64,
}

const CONNECTIVITY_URL: &str = "https://www.baidu.com";
const WEATHER_URL: &str = "https://api.open-meteo.com/v1/forecast";
const GEOCODE_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const IP_LOCATE_URL: &str = "http://ip-api.com/json/";
const QUOTE_URL: &str = "https://v1.hitokoto.cn/";
const USER_AGENT: &str = "x-hub/0.1 (local-first desktop dashboard)";

/// 中国主要城市本地经纬度表（城市名, 省, 纬度, 经度）。
/// 本地优先：中文城市名精确匹配，规避 Open-Meteo geocoding 对中文名
/// 返回同名乡级小地方的问题（如「佛山」被匹配到云南/四川的佛山）。
const CITY_DB: &[(&str, &str, f64, f64)] = &[
    // 直辖市
    ("北京", "北京", 39.90, 116.40),
    ("上海", "上海", 31.23, 121.47),
    ("天津", "天津", 39.13, 117.20),
    ("重庆", "重庆", 29.56, 106.55),
    // 河北
    ("石家庄", "河北", 38.04, 114.51),
    ("唐山", "河北", 39.63, 118.18),
    ("保定", "河北", 38.87, 115.46),
    ("廊坊", "河北", 39.54, 116.68),
    ("秦皇岛", "河北", 39.94, 119.60),
    ("邯郸", "河北", 36.63, 114.54),
    // 山西
    ("太原", "山西", 37.87, 112.55),
    ("大同", "山西", 40.08, 113.30),
    // 内蒙古
    ("呼和浩特", "内蒙古", 40.84, 111.75),
    ("包头", "内蒙古", 40.66, 109.84),
    // 辽宁
    ("沈阳", "辽宁", 41.80, 123.43),
    ("大连", "辽宁", 38.91, 121.61),
    ("鞍山", "辽宁", 41.11, 122.99),
    // 吉林
    ("长春", "吉林", 43.82, 125.32),
    ("吉林", "吉林", 43.84, 126.55),
    // 黑龙江
    ("哈尔滨", "黑龙江", 45.80, 126.53),
    ("大庆", "黑龙江", 46.59, 125.10),
    // 江苏
    ("南京", "江苏", 32.06, 118.80),
    ("苏州", "江苏", 31.30, 120.58),
    ("无锡", "江苏", 31.49, 120.31),
    ("常州", "江苏", 31.81, 119.97),
    ("南通", "江苏", 31.98, 120.89),
    ("徐州", "江苏", 34.26, 117.19),
    ("扬州", "江苏", 32.39, 119.41),
    ("镇江", "江苏", 32.19, 119.45),
    ("泰州", "江苏", 32.46, 119.92),
    ("盐城", "江苏", 33.35, 120.16),
    ("淮安", "江苏", 33.61, 119.02),
    ("连云港", "江苏", 34.60, 119.22),
    ("宿迁", "江苏", 33.96, 118.28),
    // 浙江
    ("杭州", "浙江", 30.27, 120.16),
    ("宁波", "浙江", 29.87, 121.55),
    ("温州", "浙江", 28.00, 120.67),
    ("绍兴", "浙江", 30.00, 120.58),
    ("嘉兴", "浙江", 30.75, 120.76),
    ("湖州", "浙江", 30.89, 120.09),
    ("金华", "浙江", 29.08, 119.65),
    ("台州", "浙江", 28.66, 121.42),
    ("衢州", "浙江", 28.94, 118.87),
    ("丽水", "浙江", 28.45, 119.92),
    ("舟山", "浙江", 30.00, 122.21),
    // 安徽
    ("合肥", "安徽", 31.82, 117.23),
    ("芜湖", "安徽", 31.35, 118.43),
    ("蚌埠", "安徽", 32.92, 117.39),
    // 福建
    ("福州", "福建", 26.07, 119.30),
    ("厦门", "福建", 24.48, 118.09),
    ("泉州", "福建", 24.87, 118.68),
    ("漳州", "福建", 24.51, 117.65),
    // 江西
    ("南昌", "江西", 28.68, 115.86),
    ("赣州", "江西", 25.83, 114.93),
    ("九江", "江西", 29.71, 116.00),
    // 山东
    ("济南", "山东", 36.65, 117.12),
    ("青岛", "山东", 36.07, 120.38),
    ("烟台", "山东", 37.46, 121.45),
    ("潍坊", "山东", 36.71, 119.16),
    ("临沂", "山东", 35.10, 118.36),
    ("淄博", "山东", 36.81, 118.05),
    ("威海", "山东", 37.51, 122.12),
    ("济宁", "山东", 35.42, 116.59),
    ("东营", "山东", 37.43, 118.67),
    // 河南
    ("郑州", "河南", 34.75, 113.63),
    ("洛阳", "河南", 34.62, 112.45),
    ("开封", "河南", 34.80, 114.31),
    ("南阳", "河南", 33.00, 112.53),
    ("新乡", "河南", 35.30, 113.93),
    // 湖北
    ("武汉", "湖北", 30.59, 114.31),
    ("宜昌", "湖北", 30.69, 111.29),
    ("襄阳", "湖北", 32.01, 112.12),
    ("荆州", "湖北", 30.33, 112.24),
    // 湖南
    ("长沙", "湖南", 28.23, 112.94),
    ("株洲", "湖南", 27.83, 113.13),
    ("湘潭", "湖南", 27.83, 112.94),
    ("衡阳", "湖南", 26.89, 112.57),
    // 广东
    ("广州", "广东", 23.13, 113.26),
    ("深圳", "广东", 22.54, 114.06),
    ("佛山", "广东", 23.03, 113.12),
    ("东莞", "广东", 23.02, 113.75),
    ("珠海", "广东", 22.27, 113.58),
    ("中山", "广东", 22.52, 113.39),
    ("惠州", "广东", 23.11, 114.42),
    ("江门", "广东", 22.58, 113.08),
    ("肇庆", "广东", 23.05, 112.47),
    ("汕头", "广东", 23.35, 116.68),
    ("湛江", "广东", 21.27, 110.36),
    ("茂名", "广东", 21.66, 110.93),
    ("梅州", "广东", 24.29, 116.12),
    ("清远", "广东", 23.68, 113.06),
    ("韶关", "广东", 24.81, 113.60),
    ("揭阳", "广东", 23.55, 116.37),
    ("潮州", "广东", 23.66, 116.62),
    ("汕尾", "广东", 22.79, 115.38),
    ("阳江", "广东", 21.86, 111.98),
    ("河源", "广东", 23.74, 114.70),
    ("云浮", "广东", 22.92, 112.04),
    // 广西
    ("南宁", "广西", 22.82, 108.32),
    ("桂林", "广西", 25.28, 110.29),
    ("柳州", "广西", 24.33, 109.43),
    // 海南
    ("海口", "海南", 20.04, 110.20),
    ("三亚", "海南", 18.25, 109.51),
    // 四川
    ("成都", "四川", 30.57, 104.07),
    ("绵阳", "四川", 31.47, 104.68),
    ("德阳", "四川", 31.13, 104.40),
    ("宜宾", "四川", 28.75, 104.64),
    // 贵州
    ("贵阳", "贵州", 26.65, 106.63),
    ("遵义", "贵州", 27.73, 106.93),
    // 云南
    ("昆明", "云南", 25.04, 102.71),
    ("大理", "云南", 25.61, 100.27),
    ("丽江", "云南", 26.86, 100.23),
    // 陕西
    ("西安", "陕西", 34.34, 108.94),
    ("咸阳", "陕西", 34.33, 108.71),
    ("宝鸡", "陕西", 34.36, 107.24),
    // 甘肃
    ("兰州", "甘肃", 36.06, 103.83),
    ("天水", "甘肃", 34.58, 105.72),
    // 青海
    ("西宁", "青海", 36.62, 101.78),
    // 宁夏
    ("银川", "宁夏", 38.49, 106.23),
    // 新疆
    ("乌鲁木齐", "新疆", 43.83, 87.62),
    // 西藏
    ("拉萨", "西藏", 29.65, 91.14),
    // 港澳台
    ("香港", "香港", 22.32, 114.17),
    ("澳门", "澳门", 22.20, 113.55),
    ("台北", "台湾", 25.03, 121.57),
    ("高雄", "台湾", 22.62, 120.31),
];

/// 归一化城市名：去掉「市/省/自治区/特别行政区/地区/州/盟/县/区」等行政后缀。
fn normalize_city_name(name: &str) -> String {
    let trimmed = name.trim();
    for suffix in ["特别行政区", "自治区", "自治州", "地区", "市", "省", "州", "盟", "县", "区"] {
        if trimmed.ends_with(suffix) && trimmed.len() > suffix.len() {
            return trimmed[..trimmed.len() - suffix.len()].to_string();
        }
    }
    trimmed.to_string()
}

/// 在本地城市表中查找（精确优先，其次前缀/包含）。
fn lookup_city(name: &str) -> Option<(&'static str, f64, f64)> {
    let n = normalize_city_name(name);
    if n.is_empty() {
        return None;
    }
    for &(city, _province, lat, lng) in CITY_DB {
        if city == n {
            return Some((city, lat, lng));
        }
    }
    for &(city, _province, lat, lng) in CITY_DB {
        if n.starts_with(city) || city.starts_with(&n) {
            return Some((city, lat, lng));
        }
    }
    None
}

fn client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {}", e))
}

/// 外网连通性探活：请求百度首页（国内稳定，3s 超时），2xx 即视为在线。
///
/// 刻意用「外网可达性」而非系统网络状态——内网（有局域网/网关但上不了外网）
/// 会被系统报告为已联网，但访问不了在线服务；探活才是与需求对齐的判断。
pub async fn check_connectivity() -> bool {
    match client(3) {
        Ok(c) => match c.get(CONNECTIVITY_URL).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// 用经纬度请求 Open-Meteo 当前天气。
pub async fn fetch_weather(lat: f64, lng: f64, city: &str) -> Result<WeatherCurrent, String> {
    let c = client(10)?;
    let resp = c
        .get(WEATHER_URL)
        .query(&[
            ("latitude", lat.to_string()),
            ("longitude", lng.to_string()),
            (
                "current",
                "temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m"
                    .to_string(),
            ),
            ("timezone", "auto".to_string()),
            ("forecast_days", "1".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("天气请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("天气接口返回 {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("天气响应解析失败: {}", e))?;
    let cur = &body["current"];
    Ok(WeatherCurrent {
        temperature: cur["temperature_2m"].as_f64().unwrap_or_default(),
        apparent_temperature: cur["apparent_temperature"].as_f64().unwrap_or_default(),
        relative_humidity: cur["relative_humidity_2m"].as_f64().unwrap_or_default(),
        wind_speed: cur["wind_speed_10m"].as_f64().unwrap_or_default(),
        weather_code: cur["weather_code"].as_i64().unwrap_or_default(),
        city: city.to_string(),
    })
}

/// 城市名 → 经纬度：优先本地城市表（中文名精确匹配），未命中再 fallback Open-Meteo。
pub async fn geocode_city(name: &str) -> Result<GeoLocation, String> {
    if let Some((city, lat, lng)) = lookup_city(name) {
        return Ok(GeoLocation {
            name: city.to_string(),
            lat,
            lng,
        });
    }
    geocode_remote(name).await
}

/// fallback：Open-Meteo geocoding（取第一个结果，中文名匹配可能不可靠）。
async fn geocode_remote(name: &str) -> Result<GeoLocation, String> {
    let c = client(10)?;
    let resp = c
        .get(GEOCODE_URL)
        .query(&[
            ("name", name.to_string()),
            ("count", "1".to_string()),
            ("language", "zh".to_string()),
            ("format", "json".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("地理编码请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("地理编码接口返回 {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("地理编码解析失败: {}", e))?;

    let first = body["results"]
        .as_array()
        .and_then(|r| r.first())
        .ok_or_else(|| format!("未找到城市「{}」", name))?;
    let lat = first["latitude"].as_f64().ok_or("城市纬度缺失")?;
    let lng = first["longitude"].as_f64().ok_or("城市经度缺失")?;

    let city_name = first["name"].as_str().unwrap_or(name).to_string();
    let admin = first["admin1"].as_str().map(|s| s.to_string());
    let country = first["country"].as_str().map(|s| s.to_string());
    // 展示名：省 + 国（不同时），避免「北京·北京」之类的重复
    let display = match (admin, country) {
        (Some(a), Some(c)) if a != c => format!("{}·{}", a, c),
        (Some(a), _) => a,
        (_, Some(c)) => c,
        _ => city_name.clone(),
    };
    Ok(GeoLocation {
        name: display,
        lat,
        lng,
    })
}

/// IP → 经纬度（ip-api.com 免费版，一次性定位，结果本地固化）。
pub async fn ip_locate() -> Result<GeoLocation, String> {
    let c = client(10)?;
    let resp = c
        .get(IP_LOCATE_URL)
        .query(&[(
            "fields",
            "status,message,lat,lon,city,countryCode".to_string(),
        )])
        .send()
        .await
        .map_err(|e| format!("IP 定位请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("IP 定位接口返回 {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("IP 定位解析失败: {}", e))?;

    if body["status"].as_str() != Some("success") {
        return Err(
            body["message"]
                .as_str()
                .unwrap_or("IP 定位失败")
                .to_string(),
        );
    }
    let lat = body["lat"].as_f64().ok_or("纬度缺失")?;
    let lng = body["lon"].as_f64().ok_or("经度缺失")?;
    let city = body["city"].as_str().unwrap_or("").to_string();
    let country = body["countryCode"].as_str().unwrap_or("").to_string();
    let name = match (city.is_empty(), country.is_empty()) {
        (false, false) => format!("{}·{}", city, country),
        (false, true) => city,
        (true, false) => country,
        _ => "未知位置".to_string(),
    };
    Ok(GeoLocation { name, lat, lng })
}

/// hitokoto 随机名言（encode=json，免 key）。
pub async fn fetch_quote() -> Result<Quote, String> {
    let c = client(8)?;
    let resp = c
        .get(QUOTE_URL)
        .query(&[("encode", "json")])
        .send()
        .await
        .map_err(|e| format!("名言请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("名言接口返回 {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("名言解析失败: {}", e))?;

    let content = body["hitokoto"].as_str().unwrap_or_default().trim().to_string();
    if content.is_empty() {
        return Err("名言接口返回空内容".to_string());
    }
    let from = body["from"].as_str().unwrap_or("").trim().to_string();
    Ok(Quote { content, from })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_struct_roundtrip() {
        let q = Quote {
            content: "日拱一卒，功不唐捐。".to_string(),
            from: "网络".to_string(),
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains("日拱一卒"));
        assert!(json.contains("网络"));
    }
}
