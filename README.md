# Rust Kernel (x86_64)

Простое ядро операционной системы на Rust для архитектуры x86_64 (64-bit).

## Возможности

### Базовые компоненты
- **GDT/IDT** - сегментация и обработка прерываний (64-bit)
- **Long Mode** - полноценный 64-битный режим с 4-level paging
- **Управление памятью** - собственный аллокатор кучи (2 MB), 4-level page tables
- **Многозадачность** - round-robin планировщик на 8 задач с переключением контекста по таймеру
- **VGA драйвер** - текстовый режим 80x25 с поддержкой scrollback буфера (500 строк)

### Сетевой стек
- **RTL8139** - драйвер сетевой карты Realtek (полная поддержка TX/RX)
- **Ethernet** - парсинг и создание фреймов
- **ARP** - разрешение MAC-адресов
- **IPv4** - базовая поддержка IP с checksum
- **ICMP** - ping (Echo Request/Reply)
- **UDP** - транспортный протокол
- **TCP** - полный TCP-стек: тройное рукопожатие, отправка/приём данных, FIN-закрытие, ретрансмит-таймер
- **DHCP клиент** - автоматическое получение IP-адреса при загрузке

### Файловая система
- **RamFS** - простая файловая система в памяти (create/read/write/delete)
- **ext3** - поддержка чтения и записи (superblock, inodes, bitmap allocation)
- **ATA PIO** - драйвер IDE-диска (чтение/запись секторов, 28-bit LBA)
- **Framebuffer** - графический режим через Multiboot2 (шрифт 8×16, команда `fb`)

### Shell
Интерактивная командная оболочка с историей команд (Up/Down):
- `help` - список команд
- `ls` - список файлов
- `touch/cat/write/rm` - работа с файлами
- `edit <file>` - текстовый редактор (Esc=выход, Ctrl+S=сохранить)
- `calc <expr>` - калькулятор (поддержка +, -, *, /, %, скобки)
- `mem` - статистика кучи
- `cpuinfo` - информация о процессоре
- `netinfo` - статус сетевой карты и IP конфигурация
- `dhcp` - запрос IP через DHCP
- `ping <ip>` - ICMP ping (например: ping 10.0.2.2)
- `arpwhois` - ARP запрос к gateway
- `spawn <name>` - создать задачу
- `ps` - список задач
- `uptime` - время работы системы
- `env` - системная информация
- `shutdown` - выключение
- `tcplisten <port>` - слушать TCP-порт
- `tcpconnect <ip> <port>` - подключиться к TCP-серверу
- `tcpsend <idx> <data>` - отправить данные
- `tcprecv <idx>` - принять данные
- `tcpclose <idx>` - закрыть соединение
- `tcpstat` - список TCP-соединений
- `ext3ls [path]` - список файлов на ext3
- `ext3cat <path>` - чтение файла с ext3
- `ext3info` - информация о ext3
- `fb` - информация о графическом режиме
- `httpget <ip> <port> [path]` - HTTP GET запрос
- `PgUp`/`PgDn` - скроллинг терминала

## Сборка

### Требования
- Rust nightly (GNU toolchain для Windows)
- QEMU (для запуска)
- WSL с GRUB (для создания ISO, опционально)

### Компиляция
```bash
.\build.ps1
```

### Запуск в QEMU
```bash
.\run-qemu.cmd
```

Или с ISO:
```bash
.\mkiso.ps1
.\run-qemu.cmd -iso
```

## Архитектура

### Память
- **Heap**: 2 MB, linked list allocator
- **Stack**: 32 KB на задачу
- **Paging**: 4-level page tables, identity mapping первого 1GB с 2MB страницами

### Прерывания
- IRQ0 (Timer) - переключение задач
- IRQ1 (Keyboard) - ввод с клавиатуры
- IRQ12 (Mouse) - мышь (частично)
- Exceptions - Page Fault, GPF, Double Fault и т.д.

### Сеть
- Поддержка RTL8139 (QEMU: `-net nic,model=rtl8139 -net user`)
- TX: 4 дескриптора с per-descriptor буферами (lock-free)
- RX: кольцевой буфер 8KB с volatile reads
- Автоматический DHCP при загрузке (fallback: 10.0.2.15)
- ICMP ping для проверки связи

Подробнее: [NETWORK_GUIDE.md](NETWORK_GUIDE.md)

## Изменения в x86_64 версии
- ✅ 64-битные регистры (RAX, RBX, RCX, RDX, RSI, RDI, RBP, RSP, R8-R15)
- ✅ Long mode с 4-level paging (PML4 → PDPT → PD → PT)
- ✅ 2MB huge pages для первого 1GB
- ✅ Расширенный GDT с 16-байтным TSS дескриптором
- ✅ 16-байтные IDT дескрипторы
- ✅ Multiboot2 заголовок
- ✅ Увеличенные стеки задач (32KB вместо 16KB)

## Известные ограничения
- Нет userspace (всё в ring 0)
- TCP без congestion control, Nagle, SACK (базовая реализация)
- ext3 без журнала (ext2-совместимый режим записи)
- Scheduler без приоритетов
- Сеть работает только в QEMU user mode (NAT)

## Лицензия
MIT
