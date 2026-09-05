<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.cheviiot.bastle.svg" width="128" alt="Значок Bastle">
  <h1>Bastle</h1>
  <p><strong>Превращает сайты в изолированные приложения GNOME.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/bastle/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/Cheviiot/bastle/actions/workflows/ci.yml/badge.svg"></a>
    <a href="COPYING"><img alt="Лицензия GPL-3.0-only" src="https://img.shields.io/badge/license-GPL--3.0--only-3A7D78"></a>
    <img alt="Flatpak" src="https://img.shields.io/badge/package-Flatpak-344955">
  </p>
</div>

Bastle создаёт для каждого веб-приложения отдельные ярлык, настройки, файлы cookie,
профиль и кэш. По умолчанию используется WebKitGTK, а для несовместимых сайтов
в тот же Flatpak встроен Chromium. Движок выбирает пользователь; сессии между
профилями не переносятся.

Слово *bastle* означает отдельно стоящий укреплённый дом: каждый сайт получает
своё небольшое пространство. Это дружелюбный менеджер веб-приложений, а не
обещание абсолютной безопасности сайтов.

## Установка

```sh
flatpak install --user https://cheviiot.github.io/bastle/bastle.flatpakref
```

Команда добавит подписанный удалённый репозиторий `bastle` и установит
`io.github.cheviiot.bastle`. Новые версии будут приходить через обычный
`flatpak update`. Готовые пакеты для `x86_64` и `aarch64` также публикуются в
[GitHub Releases](https://github.com/Cheviiot/bastle/releases).

## Возможности

- Создание приложения из любого корректного HTTP(S)-адреса, даже без сети.
- Отдельный профиль WebKitGTK или встроенного Chromium для каждого сайта.
- Всплывающие окна и OAuth, разрешения, уведомления, загрузки, масштаб и полноэкранный режим.
- Опциональные правила навигации, прокси, фоновый режим и фильтры содержимого.
- Зашифрованные резервные копии с предварительным просмотром и безопасным восстановлением.
- Интеграция с рабочим столом через порталы без широкого доступа к файловой системе хоста.

## Движки

| Движок | Для чего | Особенности |
| --- | --- | --- |
| WebKitGTK | Большинство сайтов и нативная интеграция с GNOME | Используется по умолчанию |
| Chromium | Сайты, несовместимые с WebKitGTK | Уже встроен; включается только после подтверждения |

DRM/Widevine, расширения браузера, обход антибот-защиты и закрытые браузерные API не
гарантируются. Bastle показывает понятную диагностику и не переключает движок
самостоятельно.

## Проект

- [Сборка и участие в разработке](CONTRIBUTING.md)
- [Модель безопасности](docs/threat-model.md)
- [Flatpak-репозиторий и ключ подписи](packaging/README.md)
- [История изменений](CHANGELOG.md)

Bastle — независимое продолжение
[Spider](https://github.com/Zaedus/spider) от коммита
`dcf9d1080ce2bbd89c342b4766a94e18aaecf660`. Авторство исходной работы Zaedus,
вклад Cameron Radmore и Git-история сохранены; прежние авторы не представлены
как участники или сторонники Bastle. Текущий код распространяется по лицензии
`GPL-3.0-only`. Подробнее: [NOTICE](NOTICE), [AUTHORS.md](AUTHORS.md) и
[COPYING](COPYING).
