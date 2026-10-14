#pragma once

#include <string>

/* The naming convention: nodes and fields in snake_case, groups and the rest in PascalCase,
 * with words split by Rant's own rule. */
namespace util {

/* A name in PascalCase: arm_controller is ArmController. */
std::string pascal(const std::string& name);

/* What a group is called: PascalCase ending in Group, so nav is NavGroup. */
std::string group_name(const std::string& name);

}
