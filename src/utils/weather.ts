// WMO weather interpretation codes（Open-Meteo weather_code）→ 中文描述 + 图标 key。
// 图标 key 由展示组件映射到具体 lucide 图标。

export interface WeatherDisplay {
  label: string
  icon: string
}

export function describeWeather(code: number): WeatherDisplay {
  if (code === 0) return { label: '晴', icon: 'sun' }
  if (code === 1) return { label: '基本晴朗', icon: 'sun' }
  if (code === 2) return { label: '多云', icon: 'cloud-sun' }
  if (code === 3) return { label: '阴', icon: 'cloud' }
  if (code === 45 || code === 48) return { label: '雾', icon: 'cloud-fog' }
  if (code >= 51 && code <= 57) return { label: '毛毛雨', icon: 'cloud-drizzle' }
  if (code >= 61 && code <= 67) return { label: '雨', icon: 'cloud-rain' }
  if (code >= 71 && code <= 77) return { label: '雪', icon: 'cloud-snow' }
  if (code >= 80 && code <= 82) return { label: '阵雨', icon: 'cloud-rain' }
  if (code === 85 || code === 86) return { label: '阵雪', icon: 'cloud-snow' }
  if (code >= 95) return { label: '雷暴', icon: 'cloud-lightning' }
  return { label: '未知', icon: 'cloud' }
}
