#pragma once

#include "SlimDetours.NDK.inl"

/*
 * Run Once
 * Similar to RtlRunOnce* but sync only and inline,
 * taken from KNSoft.MakeLifeEasier library
 */

typedef DECLSPEC_ALIGN(SIZE_OF_POINTER) struct _PS_RUNONCE
{
    DECLSPEC_ALIGN(SIZE_OF_POINTER) _Interlocked_operand_ PVOID volatile Ptr;
} PS_RUNONCE, *PPS_RUNONCE;

#define PS_RUNONCE_INIT { NULL }

#define PS_RUNONCE_STATE_MASK       0b11
#define PS_RUNONCE_STATE_INIT       0b00
#define PS_RUNONCE_STATE_PENDING    0b01
#define PS_RUNONCE_STATE_COMPLETED  0b10

C_ASSERT(PS_RUNONCE_STATE_MASK < 4);

FORCEINLINE
LOGICAL
PS_RunOnceBegin(
    _Inout_ PPS_RUNONCE RunOnce)
{
    PPS_RUNONCE Value;
    ULONG_PTR Next, State;

_Start:
    Value = RunOnce->Ptr;
    State = (ULONG_PTR)Value & PS_RUNONCE_STATE_MASK;
    if (State == PS_RUNONCE_STATE_INIT)
    {
        if (_InterlockedCompareExchangePointer(&RunOnce->Ptr,
                                               (PVOID)PS_RUNONCE_STATE_PENDING,
                                               (PVOID)PS_RUNONCE_STATE_INIT) == (PVOID)PS_RUNONCE_STATE_INIT)
        {
            return TRUE;
        }
    } else if (State == PS_RUNONCE_STATE_PENDING)
    {
        Next = (ULONG_PTR)Value & ~PS_RUNONCE_STATE_MASK;
        if (_InterlockedCompareExchangePointer(&RunOnce->Ptr,
                                               (PVOID)((ULONG_PTR)&Next | PS_RUNONCE_STATE_PENDING),
                                               Value) == Value)
        {
            NtWaitForKeyedEvent(NULL, &Next, FALSE, NULL);
        }
    } else if (State == PS_RUNONCE_STATE_COMPLETED)
    {
        return FALSE;
    } else
    {
        __fastfail(FAST_FAIL_INVALID_ARG);
    }
    goto _Start;
}

FORCEINLINE
VOID
PS_RunOnceEnd(
    _Inout_ PPS_RUNONCE RunOnce,
    _In_ LOGICAL Complete)
{
    PPS_RUNONCE Next, Value;

_Start:
    Value = RunOnce->Ptr;
    if (((ULONG_PTR)Value & PS_RUNONCE_STATE_MASK) == PS_RUNONCE_STATE_PENDING)
    {
        if (_InterlockedCompareExchangePointer((PVOID*)&RunOnce->Ptr,
                                               (PVOID)(ULONG_PTR)(Complete ? PS_RUNONCE_STATE_COMPLETED : PS_RUNONCE_STATE_INIT),
                                               Value) == Value)
        {
            Value = (PPS_RUNONCE)((ULONG_PTR)Value & ~PS_RUNONCE_STATE_MASK);
            while (Value != NULL)
            {
                Next = Value->Ptr;
                NtReleaseKeyedEvent(NULL, Value, FALSE, NULL);
                Value = Next;
            }
            return;
        }
    } else
    {
        __fastfail(FAST_FAIL_INVALID_ARG);
    }
    goto _Start;
}
