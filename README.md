<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.cheviiot.bastle.svg" width="144" alt="Значок Bastle">
  <h1>Bastle</h1>
  <p><strong>Любой сайт — отдельное приложение.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/bastle/actions/workflows/ci.yml"><img alt="Сборка" src="https://img.shields.io/github/actions/workflow/status/Cheviiot/bastle/ci.yml?branch=main&amp;style=flat-square&amp;label=сборка"></a>
    <a href="https://github.com/Cheviiot/bastle/releases"><img alt="Последний выпуск" src="https://img.shields.io/github/v/release/Cheviiot/bastle?display_name=tag&amp;sort=semver&amp;style=flat-square&amp;label=релиз&amp;color=8f7358"></a>
    <a href="COPYING"><img alt="Лицензия GPL-3.0-only" src="https://img.shields.io/badge/лицензия-GPL--3.0--only-6f7782?style=flat-square"></a>
    <img alt="Flatpak" src="https://img.shields.io/badge/пакет-Flatpak-1c1d22?style=flat-square">
  </p>
</div>

Bastle превращает любой корректный HTTP(S)-адрес в самостоятельное приложение
для рабочего стола. У каждого сайта свои настройки, профиль, файлы cookie и кэш —
без общей браузерной оболочки и смешивания сессий.

## Установка

```sh
flatpak install --user https://cheviiot.github.io/bastle/bastle.flatpakref
```

Команда установит Bastle и добавит подписанный репозиторий обновлений. Дальше
достаточно обычного `flatpak update`. Готовые пакеты для `x86_64` и `aarch64`
также доступны в [GitHub Releases](https://github.com/Cheviiot/bastle/releases).

## Что умеет Bastle

- Создаёт приложения даже без доступных названия, иконки или сети.
- Изолирует данные и профиль каждого сайта.
- Поддерживает OAuth-окна, разрешения, уведомления и загрузки.
- Управляет навигацией, прокси, фоновым режимом и фильтрами содержимого.
- Создаёт переносимые резервные копии и шифрует архивы с данными сайтов.
- Работает через системные порталы с минимальными разрешениями Flatpak.

## Два движка

- **WebKitGTK** — нативный для GNOME и используемый по умолчанию.
- **Chromium** — дополнение для сайтов, которым недостаточно WebKitGTK.

Chromium поставляется отдельным Flatpak-дополнением из того же репозитория и
не устанавливается автоматически. После установки дополнения и перезапуска
Bastle появляется выбор движка. Приложение никогда не меняет движок без
подтверждения; профили, cookies и авторизованные сессии движков не смешиваются.

## Границы совместимости

DRM/Widevine, расширения браузера, обход антибот-защиты и закрытые браузерные
API не поддерживаются и не входят в обещания совместимости проекта.

## Подробнее

[Участие в разработке](CONTRIBUTING.md) ·
[Модель безопасности](docs/threat-model.md) ·
[Репозиторий Flatpak](packaging/README.md) ·
[История изменений](CHANGELOG.md)

---

<sub>Bastle — независимое продолжение
<a href="https://github.com/Zaedus/spider">Spider</a> от коммита
<code>dcf9d1080ce2bbd89c342b4766a94e18aaecf660</code>. Исходное авторство и
Git-история сохранены; прежние авторы не участвуют в Bastle и не выражали его
одобрения. Лицензия — GPL-3.0-only. Подробности:
<a href="NOTICE">NOTICE</a>, <a href="AUTHORS.md">AUTHORS.md</a> и
<a href="COPYING">COPYING</a>.</sub>
