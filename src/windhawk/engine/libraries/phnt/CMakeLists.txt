cmake_minimum_required(VERSION 3.5)

project(phnt)

add_library(phnt INTERFACE)
target_include_directories(phnt INTERFACE "${CMAKE_CURRENT_LIST_DIR}")
target_link_libraries(phnt INTERFACE "ntdll.lib")

add_library(phnt::phnt ALIAS phnt)