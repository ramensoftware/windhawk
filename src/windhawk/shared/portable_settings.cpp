#include "stdafx.h"

#include "portable_settings.h"

// Use WIL to throw exceptions if possible.
#ifdef THROW_WIN32
#define PORTABLE_SETTINGS_THROW_WIN32(error) THROW_WIN32(error)
#else
#define PORTABLE_SETTINGS_THROW_WIN32(error) \
    throw PortableSettingsException(error)
#endif

////////////////////////////////////////////////////////////////////////////////
// EnumIteratorImpl

template <typename Type>
class EnumIteratorImpl {
   public:
    bool is_done() const { return done; }

    const std::pair<std::wstring, Type>& get_item() const { return item; }

    virtual void next() = 0;
    virtual std::unique_ptr<EnumIteratorImpl> clone() const = 0;
    virtual ~EnumIteratorImpl() = default;

   protected:
    EnumIteratorImpl() = default;
    EnumIteratorImpl(const EnumIteratorImpl&) = default;
    EnumIteratorImpl(EnumIteratorImpl&&) = default;
    EnumIteratorImpl& operator=(const EnumIteratorImpl&) = default;
    EnumIteratorImpl& operator=(EnumIteratorImpl&&) = default;

    bool done = false;
    std::pair<std::wstring, Type> item;
};

////////////////////////////////////////////////////////////////////////////////
// EnumIterator

template class PortableSettings::EnumIterator<int>;
template class PortableSettings::EnumIterator<std::wstring>;

template <typename Type>
PortableSettings::EnumIterator<Type>::EnumIterator(
    std::unique_ptr<EnumIteratorImpl<Type>> impl)
    : impl(std::move(impl)) {}

template <typename Type>
PortableSettings::EnumIterator<Type>::EnumIterator(const EnumIterator& other)
    : impl(std::move(other.impl->clone())) {}

template <typename Type>
PortableSettings::EnumIterator<Type>::EnumIterator(EnumIterator&&) noexcept =
    default;

template <typename Type>
PortableSettings::EnumIterator<Type>&
PortableSettings::EnumIterator<Type>::operator=(const EnumIterator& other) {
    impl = other.impl->clone();
    return *this;
}

template <typename Type>
PortableSettings::EnumIterator<Type>&
PortableSettings::EnumIterator<Type>::operator=(EnumIterator&&) noexcept =
    default;

template <typename Type>
PortableSettings::EnumIterator<Type>::~EnumIterator() = default;

template <typename Type>
PortableSettings::EnumIterator<Type>::operator bool() const {
    return impl->is_done();
}

template <typename Type>
PortableSettings::EnumIterator<Type>&
PortableSettings::EnumIterator<Type>::operator++() {
    impl->next();
    return *this;
}

template <typename Type>
PortableSettings::EnumIterator<Type>
PortableSettings::EnumIterator<Type>::operator++(int) {
    PortableSettings::EnumIterator copy(*this);
    ++*this;
    return copy;
}

template <typename Type>
typename PortableSettings::EnumIterator<Type>::value_type
PortableSettings::EnumIterator<Type>::operator*() const {
    return impl->get_item();
}

template <typename Type>
typename PortableSettings::EnumIterator<Type>::pointer
PortableSettings::EnumIterator<Type>::operator->() const {
    return &impl->get_item();
}

////////////////////////////////////////////////////////////////////////////////
// Helper functions - RegistrySettings

namespace {
namespace RegistrySettingsHelperFunctions {

int RawItemToInt(std::wstring data, DWORD dwDataSize, DWORD dwType) {
    int itemValue = 0;

    if (dwType == REG_DWORD && dwDataSize == sizeof(DWORD)) {
        static_assert(sizeof(int) == sizeof(DWORD));
        memcpy(&itemValue, data.data(), sizeof(DWORD));
    } else if (dwType == REG_SZ && (dwDataSize % sizeof(WCHAR)) == 0) {
        DWORD nStringSize = dwDataSize / sizeof(WCHAR) - 1;

        if (data[nStringSize] == L'\0' && wcslen(data.c_str()) == nStringSize) {
            data.resize(nStringSize);
            long longValue = std::stol(data, nullptr, 0);
            if (longValue > INT_MAX) {
                itemValue = INT_MAX;
            } else if (longValue < INT_MIN) {
                itemValue = INT_MIN;
            } else {
                itemValue = wil::safe_cast<int>(longValue);
            }
        }
    }

    return itemValue;
}

std::wstring RawItemToString(std::wstring data,
                             DWORD dwDataSize,
                             DWORD dwType) {
    std::wstring itemValue;

    if (dwType == REG_DWORD && dwDataSize == sizeof(DWORD)) {
        static_assert(sizeof(int) == sizeof(DWORD));
        int intValue;
        memcpy(&intValue, data.data(), sizeof(DWORD));
        itemValue = std::to_wstring(intValue);
    } else if (dwType == REG_SZ && (dwDataSize % sizeof(WCHAR)) == 0) {
        DWORD nStringSize = dwDataSize / sizeof(WCHAR) - 1;

        if (data[nStringSize] == L'\0' && wcslen(data.c_str()) == nStringSize) {
            data.resize(nStringSize);
            itemValue = std::move(data);
        }
    }

    return itemValue;
}

std::vector<BYTE> RawItemToBuffer(std::wstring data,
                                  DWORD dwDataSize,
                                  DWORD dwType) {
    std::vector<BYTE> itemValue;

    if (dwType == REG_BINARY) {
        auto dataBytes = reinterpret_cast<const BYTE*>(data.data());
        itemValue.assign(dataBytes, dataBytes + dwDataSize);
    }

    return itemValue;
}

}  // namespace RegistrySettingsHelperFunctions
}  // namespace

////////////////////////////////////////////////////////////////////////////////
// EnumIterator - RegistrySettings

template <typename Type>
class EnumIteratorRegistryBase : public EnumIteratorImpl<Type> {
   public:
    EnumIteratorRegistryBase(HKEY hKey) : hKey(hKey), dwIndex(0) {}

   protected:
    std::optional<std::tuple<std::wstring, std::wstring, DWORD, DWORD>>
    get_next_item_raw() {
        std::wstring valueName;
        DWORD dwValueNameSize;
        std::wstring data;
        DWORD dwDataSize;
        DWORD dwType;
        LSTATUS error;

        while (true) {
            DWORD dwMaxValueNameLen;
            DWORD dwMaxValueLen;
            error = RegQueryInfoKey(
                hKey, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,
                nullptr, &dwMaxValueNameLen, &dwMaxValueLen, nullptr, nullptr);
            if (error != ERROR_SUCCESS) {
                PORTABLE_SETTINGS_THROW_WIN32(error);
            }

            valueName.resize(wil::safe_cast<size_t>(dwMaxValueNameLen) + 1);
            dwValueNameSize = dwMaxValueNameLen + 1;
            data.resize((dwMaxValueLen + sizeof(WCHAR) - 1) / sizeof(WCHAR));
            dwDataSize = wil::safe_cast<DWORD>(data.length() * sizeof(WCHAR));
            error = RegEnumValue(
                hKey, dwIndex, &valueName[0], &dwValueNameSize, nullptr,
                &dwType, reinterpret_cast<BYTE*>(&data[0]), &dwDataSize);
            if (error == ERROR_NO_MORE_ITEMS) {
                return std::nullopt;
            }

            if (error == ERROR_MORE_DATA) {
                continue;  // perhaps value was updated, try again
            }

            if (error != ERROR_SUCCESS) {
                PORTABLE_SETTINGS_THROW_WIN32(error);
            }

            break;
        }

        dwIndex++;

        valueName.resize(dwValueNameSize);

        return std::make_tuple(std::move(valueName), std::move(data),
                               dwDataSize, dwType);
    }

    HKEY hKey;
    DWORD dwIndex;
};

class EnumIteratorRegistryInt : public EnumIteratorRegistryBase<int> {
   public:
    EnumIteratorRegistryInt(HKEY hKey) : EnumIteratorRegistryBase(hKey) {
        next();
    }

    void next() override {
        auto result = get_next_item_raw();
        if (!result) {
            done = true;
            return;
        }

        auto& [valueName, data, dwDataSize, dwType] = *result;

        int itemValue = RegistrySettingsHelperFunctions::RawItemToInt(
            std::move(data), dwDataSize, dwType);

        item = {std::move(valueName), itemValue};
    }

    std::unique_ptr<EnumIteratorImpl> clone() const override {
        return std::make_unique<EnumIteratorRegistryInt>(*this);
    }
};

class EnumIteratorRegistryString
    : public EnumIteratorRegistryBase<std::wstring> {
   public:
    EnumIteratorRegistryString(HKEY hKey) : EnumIteratorRegistryBase(hKey) {
        next();
    }

    void next() override {
        auto result = get_next_item_raw();
        if (!result) {
            done = true;
            return;
        }

        auto& [valueName, data, dwDataSize, dwType] = *result;

        std::wstring itemValue =
            RegistrySettingsHelperFunctions::RawItemToString(
                std::move(data), dwDataSize, dwType);

        item = {std::move(valueName), std::move(itemValue)};
    }

    std::unique_ptr<EnumIteratorImpl> clone() const override {
        return std::make_unique<EnumIteratorRegistryString>(*this);
    }
};

////////////////////////////////////////////////////////////////////////////////
// RegistrySettings

RegistrySettings::RegistrySettings(HKEY hKey, PCWSTR subKey, bool write) {
    LSTATUS error =
        RegCreateKeyEx(hKey, subKey, 0, nullptr, 0,
                       KEY_READ | (write ? KEY_WRITE : 0) | KEY_WOW64_64KEY,
                       nullptr, &this->hKey, nullptr);
    if (error != ERROR_SUCCESS) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

std::optional<std::wstring> RegistrySettings::GetString(
    PCWSTR valueName) const {
    auto rawData = GetRaw(valueName);
    if (!rawData) {
        return std::nullopt;
    }

    return RegistrySettingsHelperFunctions::RawItemToString(
        std::move(rawData->data), rawData->dataSize, rawData->dataType);
}

void RegistrySettings::SetString(PCWSTR valueName, PCWSTR string) {
    LSTATUS error = RegSetValueEx(
        hKey.get(), valueName, 0, REG_SZ, reinterpret_cast<const BYTE*>(string),
        wil::safe_cast<DWORD>((wcslen(string) + 1) * sizeof(WCHAR)));
    if (error != ERROR_SUCCESS) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

std::optional<int> RegistrySettings::GetInt(PCWSTR valueName) const {
    auto rawData = GetRaw(valueName);
    if (!rawData) {
        return std::nullopt;
    }

    return RegistrySettingsHelperFunctions::RawItemToInt(
        std::move(rawData->data), rawData->dataSize, rawData->dataType);
}

void RegistrySettings::SetInt(PCWSTR valueName, int value) {
    DWORD dwValue = static_cast<DWORD>(value);

    LSTATUS error =
        RegSetValueEx(hKey.get(), valueName, 0, REG_DWORD,
                      reinterpret_cast<const BYTE*>(&dwValue), sizeof(DWORD));
    if (error != ERROR_SUCCESS) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

std::optional<std::vector<BYTE>> RegistrySettings::GetBinary(
    PCWSTR valueName) const {
    auto rawData = GetRaw(valueName);
    if (!rawData) {
        return std::nullopt;
    }

    return RegistrySettingsHelperFunctions::RawItemToBuffer(
        std::move(rawData->data), rawData->dataSize, rawData->dataType);
}

void RegistrySettings::SetBinary(PCWSTR valueName,
                                 const BYTE* buffer,
                                 size_t bufferSize) {
    LSTATUS error = RegSetValueEx(hKey.get(), valueName, 0, REG_BINARY, buffer,
                                  wil::safe_cast<DWORD>(bufferSize));
    if (error != ERROR_SUCCESS) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

void RegistrySettings::Remove(PCWSTR valueName) {
    LSTATUS error = RegDeleteValue(hKey.get(), valueName);
    if (error != ERROR_SUCCESS && error != ERROR_FILE_NOT_FOUND &&
        error != ERROR_PATH_NOT_FOUND) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

RegistrySettings::EnumIterator<int> RegistrySettings::EnumIntValues() const {
    return PortableSettings::EnumIterator<int>(
        std::make_unique<EnumIteratorRegistryInt>(hKey.get()));
}

RegistrySettings::EnumIterator<std::wstring>
RegistrySettings::EnumStringValues() const {
    return PortableSettings::EnumIterator<std::wstring>(
        std::make_unique<EnumIteratorRegistryString>(hKey.get()));
}

// static
void RegistrySettings::RemoveSection(HKEY hKey, PCWSTR subKey) {
    /*
    wil::unique_hkey hKeyToDelete;
    LSTATUS error =
        RegOpenKeyEx(hKey, subKey, 0,
                     DELETE | KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE |
                         KEY_SET_VALUE | KEY_WOW64_64KEY,
                     &hKeyToDelete);
    if (error != ERROR_FILE_NOT_FOUND && error != ERROR_PATH_NOT_FOUND) {
        if (error != ERROR_SUCCESS) {
            PORTABLE_SETTINGS_THROW_WIN32(error);
        }

        error = RegDeleteTree(hKeyToDelete.get(), nullptr);
        if (error != ERROR_SUCCESS) {
            PORTABLE_SETTINGS_THROW_WIN32(error);
        }

        hKeyToDelete.reset();
    }
    */

    LSTATUS error = RegDeleteKeyEx(hKey, subKey, KEY_WOW64_64KEY, 0);
    if (error != ERROR_SUCCESS && error != ERROR_FILE_NOT_FOUND &&
        error != ERROR_PATH_NOT_FOUND) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

std::optional<RegistrySettings::RawData> RegistrySettings::GetRaw(
    PCWSTR valueName) const {
    std::wstring data;
    DWORD dataSize;
    DWORD dataType;
    LSTATUS error;

    while (true) {
        error = RegQueryValueEx(hKey.get(), valueName, nullptr, &dataType,
                                nullptr, &dataSize);
        if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) {
            return std::nullopt;
        } else if (error != ERROR_SUCCESS) {
            PORTABLE_SETTINGS_THROW_WIN32(error);
        }

        data.resize((dataSize + sizeof(WCHAR) - 1) / sizeof(WCHAR));
        dataSize = wil::safe_cast<DWORD>(data.length() * sizeof(WCHAR));
        error = RegQueryValueEx(hKey.get(), valueName, nullptr, &dataType,
                                reinterpret_cast<BYTE*>(&data[0]), &dataSize);
        if (error == ERROR_MORE_DATA) {
            continue;  // perhaps value was updated, try again
        } else if (error == ERROR_FILE_NOT_FOUND ||
                   error == ERROR_PATH_NOT_FOUND) {
            return std::nullopt;
        } else if (error != ERROR_SUCCESS) {
            PORTABLE_SETTINGS_THROW_WIN32(error);
        }

        break;
    }

    return RawData{std::move(data), dataSize, dataType};
}

////////////////////////////////////////////////////////////////////////////////
// Helper functions - IniFileSettings

namespace {
namespace IniFileSettingsHelperFunctions {

int HexDigitValue(WCHAR hexDigit) {
    if (hexDigit >= '0' && hexDigit <= '9') {
        return hexDigit - '0';
    }

    if (hexDigit >= 'A' && hexDigit <= 'F') {
        return hexDigit - 'A' + 10;
    }

    if (hexDigit >= 'a' && hexDigit <= 'f') {
        return hexDigit - 'a' + 10;
    }

    throw std::invalid_argument("invalid hex digit");
}

}  // namespace IniFileSettingsHelperFunctions
}  // namespace

////////////////////////////////////////////////////////////////////////////////
// EnumIterator - IniFileSettings

template <typename Type>
class EnumIteratorIniFileBase : public EnumIteratorImpl<Type> {
   public:
    EnumIteratorIniFileBase(const IniFileSettings* settings)
        : settings(settings) {
        for (DWORD size = 256;; size += 256) {
            SetLastError(0);

            valueNames.resize(size);
            UINT returnedSize = GetPrivateProfileString(
                settings->sectionName.c_str(), nullptr, nullptr, &valueNames[0],
                size, settings->filename.c_str());

            DWORD error = GetLastError();
            if (error == ERROR_MORE_DATA) {
                continue;  // try with a larger buffer
            } else if (error != ERROR_SUCCESS) {
                PORTABLE_SETTINGS_THROW_WIN32(error);
            }

            valueNames.resize(returnedSize);
            break;
        }
    }

   protected:
    std::optional<std::wstring> get_next_value_name() {
        size_t nextValueNameLen = wcslen(valueNames.c_str());
        if (nextValueNameLen == 0) {
            return std::nullopt;
        }

        std::wstring nextValueName = valueNames.substr(0, nextValueNameLen);
        valueNames.erase(0, nextValueNameLen + 1);

        return nextValueName;
    }

    std::optional<std::wstring> get_string(PCWSTR valueName) const {
        return settings->GetString(valueName);
    }

    std::optional<int> get_int(PCWSTR valueName) const {
        return settings->GetInt(valueName);
    }

    const IniFileSettings* settings;
    std::wstring valueNames;
};

class EnumIteratorIniFileInt : public EnumIteratorIniFileBase<int> {
   public:
    EnumIteratorIniFileInt(const IniFileSettings* settings)
        : EnumIteratorIniFileBase(settings) {
        next();
    }

    void next() override {
        auto valueName = get_next_value_name();
        if (!valueName) {
            done = true;
            return;
        }

        auto itemValue = get_int(valueName->c_str());
        if (!itemValue) {
            done = true;
            return;
        }

        item = {std::move(*valueName), *itemValue};
    }

    std::unique_ptr<EnumIteratorImpl> clone() const override {
        return std::make_unique<EnumIteratorIniFileInt>(*this);
    }
};

class EnumIteratorIniFileString : public EnumIteratorIniFileBase<std::wstring> {
   public:
    EnumIteratorIniFileString(const IniFileSettings* settings)
        : EnumIteratorIniFileBase(settings) {
        next();
    }

    void next() override {
        auto valueName = get_next_value_name();
        if (!valueName) {
            done = true;
            return;
        }

        auto itemValue = get_string(valueName->c_str());
        if (!itemValue) {
            done = true;
            return;
        }

        item = {std::move(*valueName), std::move(*itemValue)};
    }

    std::unique_ptr<EnumIteratorImpl> clone() const override {
        return std::make_unique<EnumIteratorIniFileString>(*this);
    }
};

////////////////////////////////////////////////////////////////////////////////
// IniFileSettings

IniFileSettings::IniFileSettings(PCWSTR filename,
                                 PCWSTR sectionName,
                                 bool write)
    : filename(filename), sectionName(sectionName) {
    if (write) {
        HANDLE hFile =
            CreateFile(filename, GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                       CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (hFile != INVALID_HANDLE_VALUE) {
            // Write a UTF-16LE BOM to enable Unicode.
            DWORD dwNumberOfBytesWritten;
            WriteFile(hFile, "\xFF\xFE", 2, &dwNumberOfBytesWritten, nullptr);
            CloseHandle(hFile);
        }
    }
}

std::optional<std::wstring> IniFileSettings::GetString(PCWSTR valueName) const {
    std::wstring itemValue;

    for (DWORD size = 256;; size += 256) {
        SetLastError(0);

        itemValue.resize(size);
        UINT returnedSize =
            GetPrivateProfileString(sectionName.c_str(), valueName, nullptr,
                                    &itemValue[0], size, filename.c_str());

        DWORD error = GetLastError();
        if (error == ERROR_MORE_DATA) {
            continue;  // try with a larger buffer
        } else if (error == ERROR_FILE_NOT_FOUND ||
                   error == ERROR_PATH_NOT_FOUND) {
            return std::nullopt;
        } else if (error != ERROR_SUCCESS) {
            PORTABLE_SETTINGS_THROW_WIN32(error);
        }

        itemValue.resize(returnedSize);
        break;
    }

    return itemValue;
}

void IniFileSettings::SetString(PCWSTR valueName, PCWSTR string) {
    std::wstring_view stringView{string};

    bool canBeTrimmed =
        !stringView.empty() &&
        ((stringView.front() >= L'\0' && stringView.front() <= L' ') ||
         (stringView.back() >= L'\0' && stringView.back() <= L' '));

    bool isQuoted = stringView.length() >= 2 &&
                    stringView.front() == stringView.back() &&
                    (stringView.front() == L'"' || stringView.front() == L'\'');

    bool hasNewlines = stringView.find_first_of(L"\r\n") != stringView.npos;

    std::wstring stringEscaped;
    PCWSTR stringPtr = string;

    if (canBeTrimmed || isQuoted || hasNewlines) {
        stringEscaped.reserve(stringView.length() + 2);

        if (canBeTrimmed || isQuoted) {
            stringEscaped += L'"';
        }

        if (hasNewlines) {
            for (PCWSTR p = string; *p != L'\0'; p++) {
                if (*p == L'\r') {
                    stringEscaped += L' ';
                    if (p[1] == L'\n') {
                        p++;
                    }
                } else if (*p == L'\n') {
                    stringEscaped += L' ';
                } else {
                    stringEscaped += *p;
                }
            }
        } else {
            stringEscaped += stringView;
        }

        if (canBeTrimmed || isQuoted) {
            stringEscaped += L'"';
        }

        stringPtr = stringEscaped.c_str();
    }

    SetLastError(0);

    WritePrivateProfileString(sectionName.c_str(), valueName, stringPtr,
                              filename.c_str());

    DWORD error = GetLastError();
    if (error != ERROR_SUCCESS) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

std::optional<int> IniFileSettings::GetInt(PCWSTR valueName) const {
    std::optional<std::wstring> data = GetString(valueName);
    if (!data) {
        return std::nullopt;
    }

    long longValue = std::stol(*data, nullptr, 0);
    if (longValue > INT_MAX) {
        return INT_MAX;
    } else if (longValue < INT_MIN) {
        return INT_MIN;
    }

    return wil::safe_cast<int>(longValue);
}

void IniFileSettings::SetInt(PCWSTR valueName, int value) {
    SetString(valueName, std::to_wstring(value).c_str());
}

std::optional<std::vector<BYTE>> IniFileSettings::GetBinary(
    PCWSTR valueName) const {
    std::optional<std::wstring> data = GetString(valueName);
    if (!data) {
        return std::nullopt;
    }

    // Adapted from https://stackoverflow.com/a/3382894
    const auto len = data->length();
    if (len % 2 != 0) {
        throw std::invalid_argument("odd length");
    }

    std::vector<BYTE> result;
    result.reserve(len / 2);
    for (auto it = data->begin(); it != data->end();) {
        int hi = IniFileSettingsHelperFunctions::HexDigitValue(*it++);
        int lo = IniFileSettingsHelperFunctions::HexDigitValue(*it++);
        result.push_back(hi << 4 | lo);
    }

    return result;
}

void IniFileSettings::SetBinary(PCWSTR valueName,
                                const BYTE* buffer,
                                size_t bufferSize) {
    std::wstring binaryStr;
    binaryStr.reserve(bufferSize * 2);

    static const WCHAR hexDigits[] = L"0123456789ABCDEF";

    for (const BYTE* p = buffer; p != buffer + bufferSize; p++) {
        BYTE b = *p;
        binaryStr.push_back(hexDigits[b >> 4]);
        binaryStr.push_back(hexDigits[b & 15]);
    }

    SetString(valueName, binaryStr.c_str());
}

void IniFileSettings::Remove(PCWSTR valueName) {
    SetLastError(0);

    WritePrivateProfileString(sectionName.c_str(), valueName, nullptr,
                              filename.c_str());

    DWORD error = GetLastError();
    if (error != ERROR_SUCCESS && error != ERROR_FILE_NOT_FOUND &&
        error != ERROR_PATH_NOT_FOUND) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}

IniFileSettings::EnumIterator<int> IniFileSettings::EnumIntValues() const {
    return PortableSettings::EnumIterator<int>(
        std::make_unique<EnumIteratorIniFileInt>(this));
}

IniFileSettings::EnumIterator<std::wstring> IniFileSettings::EnumStringValues()
    const {
    return PortableSettings::EnumIterator<std::wstring>(
        std::make_unique<EnumIteratorIniFileString>(this));
}

// static
void IniFileSettings::RemoveSection(PCWSTR filename, PCWSTR sectionName) {
    SetLastError(0);

    WritePrivateProfileString(sectionName, nullptr, nullptr, filename);

    DWORD error = GetLastError();
    if (error != ERROR_SUCCESS && error != ERROR_FILE_NOT_FOUND &&
        error != ERROR_PATH_NOT_FOUND) {
        PORTABLE_SETTINGS_THROW_WIN32(error);
    }
}
