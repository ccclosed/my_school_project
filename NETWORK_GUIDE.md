# 🌐 Руководство по сетевым возможностям Rust Kernel

## 📡 Поддерживаемое оборудование

### Сетевые карты
- ✅ **Realtek RTL8139** - полная поддержка (TX/RX)
- 🚧 **Intel e1000** - заглушка (в разработке)

### Протоколы
- ✅ **Ethernet** - полная поддержка
- ✅ **ARP** - Address Resolution Protocol
- ✅ **IPv4** - Internet Protocol v4
- ✅ **ICMP** - Internet Control Message Protocol (ping)
- ✅ **UDP** - User Datagram Protocol
- ✅ **DHCP** - Dynamic Host Configuration Protocol

## 🚀 Быстрый старт

### 1. Запуск ядра с сетью

```cmd
run-qemu.cmd
```

QEMU автоматически настроен с:
- Сетевой картой RTL8139
- User-mode networking (NAT)
- Доступ к хосту через 10.0.2.2

### 2. Автоматическая настройка

При загрузке ядро автоматически:
1. Обнаруживает RTL8139 на PCI шине
2. Инициализирует драйвер
3. Запрашивает IP через DHCP
4. Настраивает сеть

Если DHCP не отвечает (timeout 5 сек), используется fallback IP: **10.0.2.15**

## 📋 Сетевые команды

### `netinfo` - Информация о сети
Показывает:
- Тип сетевой карты
- MAC адрес
- Статус линка
- IP конфигурацию
- PCI информацию

```
kernel> netinfo
interface: Realtek RTL8139
mac: 52:54:00:12:34:56
link: up
ip: 10.0.2.15
gateway: 10.0.2.2
dns: 8.8.8.8
rx ring: CAPR=0 CBR=0
pci 00:03.0 - vendor 10ec device 8139
```

### `dhcp` - Запрос IP через DHCP
Вручную запрашивает IP адрес у DHCP сервера:

```
kernel> dhcp
DHCP discover (xid=0x12345678)...
DHCP OK!
IP: 10.0.2.15 / mask: 255.255.255.0 / gw: 10.0.2.2 / dns: 8.8.8.8
```

### `ping <ip>` - ICMP Echo Request
Отправляет ping на указанный IP:

```
kernel> ping 10.0.2.2
PING 10.0.2.2 from 10.0.2.15
Resolving MAC address...
Sent 18 bytes
Reply from 10.0.2.2: time=2ms
```

Поддерживает:
- Автоматическое ARP разрешение
- Timeout 5 секунд
- Прерывание через Ctrl+C

### `arpwhois` - ARP запрос к gateway
Отправляет ARP запрос к шлюзу по умолчанию:

```
kernel> arpwhois
Sending ARP who-has 10.0.2.2 ...
TX OK
Reply from 10.0.2.2 is 52:55:0a:00:02:02
```

### `looptest` - Тест loopback
Проверяет TX/RX путь через внутренний loopback:

```
kernel> looptest
Loopback ON, sending packet...
TX done, checking RX...
LOOPBACK OK: received 42 bytes!
  dst=ff:ff:... src=52:54:... ethertype=0x0806
Loopback OFF
```

### `netdiag` - Дамп регистров NIC
Выводит состояние всех регистров RTL8139 для отладки:

```
kernel> netdiag
RTL8139 registers:
  CR     = 0x0c (TE=1 RE=1 RST=0)
  TCR    = 0x000000e0
  RCR    = 0x00000f0f
  ISR    = 0x0000
  IMR    = 0x0005
  TSD0   = 0x00008000 (TOK=1 OWN=0 TER=0)
  CAPR   = 0
  CBR    = 0
  RBSTART= 0x00400000
  MAC    = 52:54:00:12:34:56
```

## 🔧 Архитектура драйвера

### RTL8139 Driver (`src/drivers/net/rtl8139.rs`)

**Особенности:**
- Lock-free TX с 4 дескрипторами (параллельная отправка)
- Кольцевой RX буфер 8KB + 16 bytes
- Volatile reads для защиты от race conditions с NIC
- Memory fence для корректной синхронизации
- Timer-based timeouts вместо spin loops

**TX Path:**
1. Выбор свободного дескриптора (round-robin)
2. Копирование данных в TX буфер
3. Запись физического адреса в TSAD
4. Запись длины в TSD (автоматически запускает TX)
5. Polling TOK бита с timeout

**RX Path:**
1. Сравнение CAPR и CBR
2. Volatile read заголовка пакета (status + size)
3. Валидация размера
4. Копирование данных с volatile reads
5. Обновление CAPR с выравниванием на 4 байта

### Network Stack

```
Application (shell commands)
         ↓
    UDP / ICMP
         ↓
       IPv4
         ↓
     Ethernet
         ↓
   RTL8139 Driver
         ↓
    Hardware NIC
```

## 🐛 Отладка сетевых проблем

### Проблема: "No NIC available"
**Причина:** PCI сканирование не нашло RTL8139  
**Решение:** Проверьте параметры QEMU: `-net nic,model=rtl8139`

### Проблема: "DHCP timeout"
**Причина:** DHCP сервер не отвечает  
**Решение:** 
- Проверьте `-net user` в QEMU
- Используется fallback IP 10.0.2.15
- Можно работать с этим IP

### Проблема: "TX failed"
**Причина:** Таймаут отправки пакета  
**Решение:**
- Запустите `netdiag` для проверки регистров
- Проверьте TSD0 регистр (TOK бит)
- Возможно переполнение TX буфера

### Проблема: "No RX data"
**Причина:** Пакеты не принимаются  
**Решение:**
- Проверьте CAPR == CBR (буфер пуст)
- Запустите `looptest` для проверки RX пути
- Проверьте RCR регистр (должен быть 0x00000f0f)

## 📊 Производительность

### Пропускная способность
- **TX:** ~10 Mbps (ограничено polling)
- **RX:** ~8 Mbps (ограничено размером буфера)

### Латентность
- **Ping RTT:** 1-5 ms (QEMU user networking)
- **ARP resolution:** 1-3 ms
- **DHCP full cycle:** 100-500 ms

## 🔮 Планы развития

### Ближайшие улучшения
- [ ] Interrupt-driven RX/TX (вместо polling)
- [ ] TCP stack (базовый)
- [ ] DNS resolver
- [ ] HTTP client (простой GET)
- [ ] Поддержка Intel e1000

### Долгосрочные цели
- [ ] IPv6 support
- [ ] TLS/SSL (mbedtls)
- [ ] WebSocket client
- [ ] NFS client для сетевой FS

## 📝 Примеры использования

### Проверка связи с хостом
```
kernel> ping 10.0.2.2
```

### Получение нового IP
```
kernel> dhcp
```

### Диагностика сети
```
kernel> netinfo
kernel> netdiag
kernel> looptest
```

### ARP таблица (manual)
```
kernel> arpwhois
```

## 🎯 QEMU User Networking

### Доступные адреса
- **10.0.2.2** - Gateway (хост)
- **10.0.2.3** - DNS сервер
- **10.0.2.15** - Гостевая система (по умолчанию)

### Ограничения
- Нет входящих соединений (только исходящие)
- ICMP ping к внешним хостам может не работать
- Некоторые протоколы могут быть ограничены

### Port forwarding (опционально)
Добавьте в run-qemu.cmd:
```cmd
-net user,hostfwd=tcp::8080-:80
```

## 🔐 Безопасность

### Текущие ограничения
- ⚠️ Нет проверки checksums в RX пути
- ⚠️ Нет защиты от ARP spoofing
- ⚠️ Нет rate limiting
- ⚠️ Promiscuous mode включен

### Рекомендации
- Используйте только в изолированной среде (QEMU)
- Не подключайте к реальной сети без доработок
- Добавьте валидацию входящих пакетов

## 📚 Ссылки

- [RTL8139 Datasheet](http://www.realtek.com.tw/products/productsView.aspx?Langid=1&PFid=5&Level=5&Conn=4&ProdID=35)
- [OSDev RTL8139](https://wiki.osdev.org/RTL8139)
- [RFC 791 - IPv4](https://tools.ietf.org/html/rfc791)
- [RFC 826 - ARP](https://tools.ietf.org/html/rfc826)
- [RFC 2131 - DHCP](https://tools.ietf.org/html/rfc2131)
