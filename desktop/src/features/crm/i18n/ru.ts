/**
 * Russian UI copy for the CRM feature (AD-005).
 *
 * Every user-visible CRM string lives here; `.tsx` files under
 * `features/crm` must not contain Cyrillic (enforced by
 * `features/crm/i18n/ru.test.mjs`). Keep keys grouped by screen.
 */
export const ru = {
  common: {
    retry: "Повторить",
    save: "Сохранить",
    saving: "Сохранение…",
    cancel: "Отмена",
    refresh: "Обновить",
    yes: "Да",
    no: "Нет",
  },
  shell: {
    title: "CRM",
    description: "Склад, клиенты, продажи и касса вашего магазина.",
    sectionsLabel: "Разделы CRM",
    refreshLabel: "Обновить данные CRM",
  },
  sections: {
    settings: "Настройки",
  },
  gate: {
    loading: "Загрузка CRM…",
    loadError: "Не удалось загрузить CRM.",
    noSpaceTitle: "CRM ещё не создана",
    noSpaceAskAdmin:
      "Попросите администратора сообщества создать CRM-пространство.",
    noAccessTitle: "Нет доступа к CRM",
    noAccessBody:
      "CRM-пространство уже создано, но вы не участник его канала. Попросите администратора добавить вас в канал CRM.",
  },
  create: {
    title: "Создать CRM-пространство",
    description:
      "Данные CRM хранятся в закрытом канале: видеть и изменять их смогут только его участники.",
    channelLegend: "Канал CRM",
    newChannel: "Создать новый закрытый канал «crm»",
    existingChannel: "Использовать существующий закрытый канал",
    channelLabel: "Канал",
    noEligibleChannels: "Нет подходящих закрытых каналов — будет создан новый.",
    warehouseLabel: "Название основного склада",
    warehouseDefault: "Основной склад",
    submit: "Создать CRM",
    submitting: "Создание…",
    newChannelName: "crm",
    newChannelDescription: "Рабочее пространство CRM",
    created: "CRM-пространство создано",
  },
  settings: {
    title: "Настройки CRM",
    readOnlyHint:
      "Изменять настройки могут только владелец и администраторы канала CRM.",
    timezoneLabel: "Часовой пояс",
    timezoneHint: "Определяет границы дня в документах и аналитике.",
    fiscalizationLabel: "Фискализация продаж",
    fiscalization: {
      required: "Обязательна",
      optional: "По желанию",
      off: "Отключена",
    },
    fiscalizationHint: {
      required: "Каждая продажа пробивается на кассе.",
      optional: "Чек можно не пробивать.",
      off: "Касса не используется.",
    },
    vatLabel: "НДС по умолчанию для новых товаров",
    negativeStockLabel: "Разрешить отрицательные остатки",
    negativeStockHint:
      "Если выключено, нельзя продать больше, чем есть на складе.",
    saved: "Настройки сохранены",
  },
  vat: {
    none: "Без НДС",
    vat0: "НДС 0%",
    vat5: "НДС 5%",
    vat7: "НДС 7%",
    vat10: "НДС 10%",
    vat20: "НДС 20% (до 2026 года)",
    vat22: "НДС 22%",
    vat105: "НДС 5/105",
    vat107: "НДС 7/107",
    vat110: "НДС 10/110",
    vat120: "НДС 20/120 (до 2026 года)",
    vat122: "НДС 22/122",
  },
  warehouses: {
    title: "Склады",
    add: "Добавить склад",
    empty: "Складов пока нет.",
    defaultBadge: "По умолчанию",
    archivedBadge: "В архиве",
    actionsLabel: "Действия со складом",
    rename: "Переименовать",
    makeDefault: "Сделать основным",
    archive: "Отправить в архив",
    createTitle: "Новый склад",
    renameTitle: "Переименовать склад",
    nameLabel: "Название склада",
    created: "Склад добавлен",
    updated: "Склад обновлён",
    archived: "Склад отправлен в архив",
  },
  errors: {
    restricted: "Недостаточно прав для этого действия.",
    conflict:
      "Данные изменились, пока вы их редактировали. Обновите и повторите.",
    invalid: "Relay отклонил данные.",
    network: "Нет связи с сервером. Проверьте подключение и повторите.",
    unknown: "Не удалось выполнить действие.",
    nameRequired: "Введите название.",
    channelRequired: "Выберите канал.",
  },
  timezones: {
    "Europe/Kaliningrad": "Калининград (UTC+2)",
    "Europe/Moscow": "Москва (UTC+3)",
    "Europe/Samara": "Самара (UTC+4)",
    "Asia/Yekaterinburg": "Екатеринбург (UTC+5)",
    "Asia/Omsk": "Омск (UTC+6)",
    "Asia/Novosibirsk": "Новосибирск (UTC+7)",
    "Asia/Krasnoyarsk": "Красноярск (UTC+7)",
    "Asia/Irkutsk": "Иркутск (UTC+8)",
    "Asia/Yakutsk": "Якутск (UTC+9)",
    "Asia/Vladivostok": "Владивосток (UTC+10)",
    "Asia/Magadan": "Магадан (UTC+11)",
    "Asia/Kamchatka": "Камчатка (UTC+12)",
  },
} as const;
