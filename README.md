# Rust Kernel (x86_64)

Операционная система на Rust для архитектуры x86_64. Загружается через Multiboot2 (GRUB / QEMU).

## Возможности

### Базовые компоненты
- **GDT/IDT** — 64-bit сегментация, TSS, обработка прерываний
- **Long Mode** — 4-level paging (PML4→PDPT→PD→PT), 2MB huge pages
- **Память** — bucket-аллокатор 2 MB (O(1)), identity-map 1GB
- **Многозадачность** — round-robin планировщик, 8 задач, таймер 1000Hz
- **VGA** — 80x25, scrollback 500 строк, цветной вывод
- **PgUp/PgDn** — скроллинг терминала

### Сетевой стек
- **RTL8139** — драйвер (TX/RX, кольцевой буфер)
- **TCP** — полный стек: рукопожатие, PSH/ACK, FIN, ретрансмит
- **HTTP** — клиент (`httpget`) поверх TCP
- **IPv4**, **ICMP** (ping), **UDP**, **ARP**, **DHCP**

### Файловая система
- **RamFS** — в памяти (create/read/write/delete)
- **ext3** — чтение и запись (superblock, inodes, bitmap)
- **ATA PIO** — драйвер IDE-диска

### Framebuffer (экспериментально)
- Bochs/QEMU LFB (0xFD000000), консоль 8x16, команда `fb`

## Команды Shell

| Команда | Описание |
|---------|----------|
| `help` | список команд |
| `ls` / `touch` / `cat` / `write` / `rm` | работа с файлами |
| `cp` / `mv` | копировать / переместить |
| `edit <f>` | редактор (Esc=выход, Ctrl+S=сохранить) |
| `calc <expr>` | калькулятор (+ - * / %) |
| `mem` / `free` | статистика памяти |
| `cpuinfo` / `lspci` | информация о CPU / PCI |
| `dhcp` | получить IP |
| `ping <ip>` / `arpwhois` | ICMP / ARP |
| `tcplisten` / `tcpconnect` / `tcpsend` / `tcprecv` / `tcpclose` | TCP-команды |
| `tcpstat` | список TCP-соединений |
| `httpget <ip> <port> [path]` | HTTP GET |
| `ext3ls` / `ext3cat` / `ext3info` | ext3 |
| `fb` | framebuffer info |
| `spawn` / `ps` | задача / список |
| `uptime` / `env` / `shutdown` | система |
| `clear` | очистить экран |
| `PgUp` / `PgDn` | скроллинг |

## Сборка и запуск

```bash
# Сборка
.\build.ps1

# Запуск
.\run-qemu.cmd          # текстовый режим + сеть
.\mkiso.ps1 && .\run-qemu.cmd -iso   # ISO-образ
```

## Тестирование HTTP и ext3

**HTTP:** на хосте `python -m http.server 8000`, в QEMU `dhcp` → `httpget 10.0.2.2 8000 /`

**ext3:** `dd if=/dev/zero of=disk.img bs=1M count=64 && mkfs.ext3 disk.img`, запуск `-drive file=disk.img,format=raw,if=ide`, в QEMU `ext3ls /`

## Известные ограничения

- Нет userspace (ring 0), нет SMP/ACPI/USB
- TCP без congestion control / Nagle / SACK
- ext3 без журнала (ext2-совместимый режим)
- Мышь нестабильна на Q35, framebuffer в разработке
- Сеть только QEMU user-mode (NAT)

## Лицензия

MIT
