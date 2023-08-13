/*
 * Copyright (c) 2016-2021 WangBin <wbsecg1 at gmail.com>
 * https://github.com/wang-bin/ThreadLocal
 * https://github.com/wang-bin/cppcompat/blob/master/include/cppcompat/thread_local.hpp
 */
#pragma once
#include <functional>
#include <memory> //default_delete
#include <thread>
#ifndef TLS_NO_DEBUG
#include <iostream>
#endif
#ifndef USE_STD_THREAD_LOCAL
#define USE_STD_THREAD_LOCAL 0 // 0: use our own implementation. 1: use c++11 thread_local if possible.
#endif

#if defined(__MINGW32__) // mingw clang does not support non-trivial destructible types. mingw gcc is still broken: https://gcc.gnu.org/bugzilla/show_bug.cgi?id=83562
// new gcc defines __has_feature but no cxx_thread_local
#elif (__clang__ + 0) // clang defines _MSC_VER as the cl builds it, or masquerades as gcc4.2, so check clang first
# if __has_feature(cxx_thread_local)
#   define CC_HAS_THREAD_LOCAL (!(_LIBCPP_VERSION + 0)/*gnu stl, vcrt*/ || _LIBCPP_VERSION >= 4000)
# endif
// apple clang: no cxx_thread_local for iOS(and macOS if xcode<8), no thread_local/__thread. opensource clang: targeting macOS 10.7+
// apple clang: always implemented in _tlv_atexit(apple/opensource) for darwin, which is available in macOS10.10+/iOS8.0+(arm64,x86_64)/iOS9.0+(armv7)
// new clang/libc++7.0 supports osx10.4+ (compiler-rt) but not ios<8.0: https://lists.llvm.org/pipermail/llvm-dev/2018-December/128364.html
// libstdc++(g++4.8+) and libc++4.0+(not apple). implemented in __cxa_thread_atexit in libc++abi, 4.0+ abi has a fallback if no __cxa_thread_atexit_impl (e.g. android<23)
#elif (_MSC_VER+0) >= 1900
# define CC_HAS_THREAD_LOCAL 1
#elif (__GNUC__*100+__GNUC_MINOR__) >= 408 // can't be clang
# define CC_HAS_THREAD_LOCAL 1
#endif

#if (CC_HAS_THREAD_LOCAL+0) && USE_STD_THREAD_LOCAL
#define THREAD_LOCAL(T) thread_local T
#else
#define THREAD_LOCAL(T) ThreadLocal<T>
#endif

// Fibers api is preferred for mingw targeting vista or later, and msvc
#if defined(_WIN32) // http://nadeausoftware.com/articles/2012/01/c_c_tip_how_use_compiler_predefined_macros_detect_operating_system
#include <windows.h>
#   if (defined(_MSC_VER) || !defined(USE_PTHREAD)) && _WIN32_WINNT >= 0x0600 // default use fibers api for mingw targeting store/vista
#       define USE_FLS // vista, winstore
#   endif
#endif
#if !defined(USE_FLS)
#include <pthread.h>
#   ifndef USE_PTHREAD
#       define USE_PTHREAD
#   endif //USE_PTHREAD
#endif

#ifndef FUNCINFO
#if defined(_MSC_VER)
#define FUNCINFO __FUNCSIG__
#else
#define FUNCINFO __PRETTY_FUNCTION__
#endif
#endif

/*!
 * differences between `static ThreadLocal<T> t` and `static thread_local T s`
 * 1. t is an object of type ThreadLocal<T>, the thread data is of type T, while s is an object of T.
 * 2. t is constructed only once, while s is constructed when a new thread starts.
 * 3. thread data of t is constructed only when it's accessed in a new thread, or when t is constructed by assignment. while s (thread data is it's self) is constructed when a new thread starts.
 * \code
 * void f() { static THREAD_LOCAL(int) a = 1; }
 * \endcode
 * int data of value 1 constructed only once (when constructing ThreadLocal using get()).
 * \code
 * void f() { static THREAD_LOCAL(int) a(1); }
 * \endcode
 * int data of value 1 is not constructed because get() is not called.
 * All the diferences above does not affect your program.
 */
template<typename T>
class ThreadLocal
{
public:
    // force cast to std::function<T*()> to fix android/ios clang ambiguous default ctor. why?
    ThreadLocal() : ThreadLocal(std::function<T*()>([]{return new T();})) {}
    ThreadLocal(const T& t) : ThreadLocal([t]{ return new T(t);}) {}
    ThreadLocal(T&& t) : ThreadLocal([t]{ return new T(std::move(t));}) {}
    ThreadLocal(std::function<T*()> c, std::function<void(T*)> d = std::default_delete<T>())
    : ctor_(c)
    , dtor_(d) {
#ifdef USE_PTHREAD
        pthread_key_create(&key_, default_exit);
#endif
#ifdef USE_FLS
        index_ = FlsAlloc(default_exit);
        if (index_ == FLS_OUT_OF_INDEXES)
            throw std::system_error(GetLastError(), std::system_category(), "FlsAlloc error");
#endif
    }
    //ThreadLocal(const ThreadLocal& t) : ThreadLocal(*t.get()) {}
    ThreadLocal(ThreadLocal&& t) : ThreadLocal() {
        t.move_get(get());
        //t = T();
    }
    ThreadLocal& operator=(ThreadLocal&& t) {
        t.move_get(get());
        //*get() = std::move(*t.get());
       // t = T();
        return *this;
    }
    ~ThreadLocal() {
#ifdef USE_PTHREAD
        pthread_key_delete(key_);
#endif
#ifdef USE_FLS
        FlsFree(index_);
#endif
    }
    T* operator&() const { return get();}
    /*!
     * The following operators let ThreadLocal behaves like c++11 thread_local var, except T's member must be accessed by operator->(),
     * e.g. t->member (a workaround is explicitly convert to access: ((T&)t).member)
     */
    operator T&() const { return *get(); }
    ThreadLocal& operator=(const T& v) {
        *get() = v;
        return *this;
    }
    ThreadLocal& operator=(T&& v) {
        *get() = std::move(v);
        return *this;
    }
private:
    void move_get(T* t) {
        void* v = nullptr;
#if defined(USE_PTHREAD)
        v = pthread_getspecific(key_);
#elif defined(USE_FLS)
        v = FlsGetValue(index_);
#endif
        if (!v)
            return;
        *t = std::move(*static_cast<Data*>(v)->t);
    }
    T* get() const {
        void* v = nullptr;
#if defined(USE_PTHREAD)
        v = pthread_getspecific(key_);
#elif defined(USE_FLS)
        v = FlsGetValue(index_);
#else
        return nullptr;
#endif
        if (v)
            return static_cast<Data*>(v)->t;
        Data *d = new Data();
#ifndef TLS_NO_DEBUG
        std::clog << FUNCINFO << " allocate and initialize ThreadLocal data" << std::endl << std::flush;
#endif
        d->t = ctor_();
        d->tl = this;
#if defined(USE_PTHREAD)
        pthread_setspecific(key_, d);
#elif defined(USE_FLS)
        FlsSetValue(index_, d);
#endif
        return d->t;
    }
    T* operator->() const { return get(); }
    static void
#ifdef USE_FLS
    WINAPI // WINAPI/__stdcall is required to avoid crash on 32bit target, ignored by x64/arm compiler
#endif
    default_exit(void* v) {
        Data* d = static_cast<Data*>(v);
        if (d && d->tl->dtor_)
            d->tl->dtor_(d->t);
        delete d;
    }

    struct Data {
#ifndef TLS_NO_DEBUG
        Data() { std::clog << FUNCINFO << " thread: " << std::this_thread::get_id() << std::endl; }
        ~Data() { std::clog << FUNCINFO << " thread: " << std::this_thread::get_id() << std::endl; }
#endif
        const ThreadLocal* tl = nullptr;
        T* t = nullptr;
    };
#ifdef USE_PTHREAD
    pthread_key_t key_;
#endif
#ifdef USE_FLS
    DWORD index_;
#endif
    // static ThreadLocal<T> var, var ctor will be called only once, so must store how var is allocated and initialized, using ctor_
    std::function<T*()> ctor_ = nullptr;
    std::function<void(T*)> dtor_ = nullptr;
};
