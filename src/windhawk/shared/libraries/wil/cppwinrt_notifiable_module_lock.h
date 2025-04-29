//*********************************************************
//
//    Copyright (c) Microsoft. All rights reserved.
//    This code is licensed under the MIT License.
//    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
//    ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
//    TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
//    PARTICULAR PURPOSE AND NONINFRINGEMENT.
//
//*********************************************************
//! @file
//! Utilities for implementing OOP COM server with cppwinrt only

#ifndef __WIL_CPPWINRT_NOTIFIABLE_MODULE_LOCK_INCLUDED
#define __WIL_CPPWINRT_NOTIFIABLE_MODULE_LOCK_INCLUDED

#ifdef WINRT_BASE_H
#error You must include this header before including any winrt header
#endif

#ifndef WINRT_CUSTOM_MODULE_LOCK
#error You must define 'WINRT_CUSTOM_MODULE_LOCK' at the project level
#endif

#include <atomic>
#include <cstdint>

namespace wil
{
// Adopted from cppwinrt
struct notifiable_module_lock_base
{
    notifiable_module_lock_base() = default;

    notifiable_module_lock_base(uint32_t count) : m_count(count)
    {
    }

    uint32_t operator=(uint32_t count) noexcept
    {
        return m_count = count;
    }

    uint32_t operator++() noexcept
    {
        return m_count.fetch_add(1, std::memory_order_relaxed) + 1;
    }

    uint32_t operator--() noexcept
    {
        auto const remaining = m_count.fetch_sub(1, std::memory_order_release) - 1;

        if (remaining == 0)
        {
            std::atomic_thread_fence(std::memory_order_acquire);
            if (notifier) // Protect against callback not being set yet
            {
                notifier();
            }
        }
        else if (remaining < 0)
        {
            abort();
        }

        return remaining;
    }

    operator uint32_t() const noexcept
    {
        return m_count;
    }

    template <typename Func>
    void set_notifier(Func&& func)
    {
        notifier = std::forward<Func>(func);
    }

    void set_notifier(std::nullptr_t) noexcept
    {
        notifier = nullptr;
    }

private:
    std::atomic<int32_t> m_count{0};
    std::function<void()> notifier{};
};

struct notifiable_module_lock final : notifiable_module_lock_base
{
    static notifiable_module_lock& instance() noexcept
    {
        static notifiable_module_lock lock;
        return lock;
    }
};
} // namespace wil

#ifndef WIL_CPPWINRT_COM_SERVER_CUSTOM_MODULE_LOCK

namespace winrt
{
auto& get_module_lock()
{
    return wil::notifiable_module_lock::instance();
}
} // namespace winrt

#endif

#endif