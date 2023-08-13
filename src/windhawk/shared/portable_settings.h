#pragma once

class PortableSettingsException : public std::exception {
    DWORD error;
    char errorMessage[sizeof("PortableSettingsException 1234567890")];

   public:
    PortableSettingsException(DWORD error) : error(error) {
        sprintf_s(errorMessage, "PortableSettingsException %u", error);
    }

    const char* what() const noexcept override { return errorMessage; }

    DWORD error_code() const noexcept { return error; }
};

template <typename Type>
class EnumIteratorImpl;

class PortableSettings {
   public:
    // Generator pattern in C++: https://stackoverflow.com/a/9060689
    template <typename Type>
    class EnumIterator {
       public:
        using iterator_category = std::input_iterator_tag;
        using value_type = std::pair<std::wstring, Type>;
        using difference_type = std::ptrdiff_t;
        using pointer = const value_type*;
        using reference = const value_type&;

        EnumIterator(std::unique_ptr<EnumIteratorImpl<Type>> impl);
        EnumIterator(const EnumIterator&);
        EnumIterator(EnumIterator&&) noexcept;
        EnumIterator& operator=(const EnumIterator&);
        EnumIterator& operator=(EnumIterator&&) noexcept;
        ~EnumIterator();
        explicit operator bool() const;
        EnumIterator& operator++();    // prefix increment
        EnumIterator operator++(int);  // postfix increment
        value_type operator*() const;
        pointer operator->() const;

       private:
        std::unique_ptr<EnumIteratorImpl<Type>> impl;
    };

    PortableSettings(const PortableSettings&) = delete;
    PortableSettings(PortableSettings&&) = delete;
    PortableSettings& operator=(const PortableSettings&) = delete;
    PortableSettings& operator=(PortableSettings&&) = delete;
    virtual ~PortableSettings() = default;
    virtual std::optional<std::wstring> GetString(PCWSTR valueName) const = 0;
    virtual void SetString(PCWSTR valueName, PCWSTR string) = 0;
    virtual std::optional<int> GetInt(PCWSTR valueName) const = 0;
    virtual void SetInt(PCWSTR valueName, int value) = 0;
    virtual std::optional<std::vector<BYTE>> GetBinary(
        PCWSTR valueName) const = 0;
    virtual void SetBinary(PCWSTR valueName,
                           const BYTE* buffer,
                           size_t bufferSize) = 0;
    virtual void Remove(PCWSTR valueName) = 0;
    virtual EnumIterator<int> EnumIntValues() const = 0;
    virtual EnumIterator<std::wstring> EnumStringValues() const = 0;

   protected:
    PortableSettings() = default;
};

class RegistrySettings : public PortableSettings {
   public:
    RegistrySettings(HKEY hKey, PCWSTR subKey, bool write);

    std::optional<std::wstring> GetString(PCWSTR valueName) const override;
    void SetString(PCWSTR valueName, PCWSTR string) override;
    std::optional<int> GetInt(PCWSTR valueName) const override;
    void SetInt(PCWSTR valueName, int value) override;
    std::optional<std::vector<BYTE>> GetBinary(PCWSTR valueName) const override;
    void SetBinary(PCWSTR valueName,
                   const BYTE* buffer,
                   size_t bufferSize) override;
    void Remove(PCWSTR valueName) override;
    EnumIterator<int> EnumIntValues() const override;
    EnumIterator<std::wstring> EnumStringValues() const override;

    static void RemoveSection(HKEY hKey, PCWSTR subKey);

   private:
    struct RawData {
        std::wstring data;
        DWORD dataSize;
        DWORD dataType;
    };

    std::optional<RawData> GetRaw(PCWSTR valueName) const;

    wil::unique_hkey hKey;
};

class IniFileSettings : public PortableSettings {
   public:
    IniFileSettings(PCWSTR filename, PCWSTR sectionName, bool write);

    std::optional<std::wstring> GetString(PCWSTR valueName) const override;
    void SetString(PCWSTR valueName, PCWSTR string) override;
    std::optional<int> GetInt(PCWSTR valueName) const override;
    void SetInt(PCWSTR valueName, int value) override;
    std::optional<std::vector<BYTE>> GetBinary(PCWSTR valueName) const override;
    void SetBinary(PCWSTR valueName,
                   const BYTE* buffer,
                   size_t bufferSize) override;
    void Remove(PCWSTR valueName) override;
    EnumIterator<int> EnumIntValues() const override;
    EnumIterator<std::wstring> EnumStringValues() const override;

    static void RemoveSection(PCWSTR filename, PCWSTR sectionName);

   private:
    template <typename Type>
    friend class EnumIteratorIniFileBase;

    std::wstring filename;
    std::wstring sectionName;
};
