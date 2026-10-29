cmake_minimum_required(VERSION 3.16)
project(appcsxcad_qt_prototype LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Widgets)

add_executable(appcsxcad-qt-prototype main.cpp)
target_link_libraries(appcsxcad-qt-prototype PRIVATE Qt6::Widgets)
